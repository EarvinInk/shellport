//! SSH connections: auth, host key verification, ProxyJump, connection pool,
//! and local port forwarding.

use crate::state::AppState;
use crate::store::hosts::{AuthMethod, Host, PortForward};
use crate::vault::SecretKind;
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use russh::client::{self, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use zeroize::Zeroizing;

pub struct Client {
    app: AppHandle,
    host: String,
    port: u16,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyPrompt {
    pub request_id: String,
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    /// true when known_hosts has a *different* key for this host.
    pub changed: bool,
    pub changed_line: Option<usize>,
}

fn known_hosts_file() -> std::path::PathBuf {
    crate::paths::home_dir().join(".ssh/known_hosts")
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let pk = key.public_key();
        let path = known_hosts_file();
        let (changed, changed_line) = match keys::check_known_hosts_path(&self.host, self.port, &pk, &path) {
            Ok(true) => return Ok(true),
            Ok(false) => (false, None),
            Err(keys::Error::KeyChanged { line }) => (true, Some(line)),
            Err(_) => (false, None), // missing/unreadable file: treat as unknown
        };
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(request_id.clone(), tx);
        let prompt = HostKeyPrompt {
            request_id: request_id.clone(),
            host: self.host.clone(),
            port: self.port,
            algorithm: pk.algorithm().as_str().to_string(),
            fingerprint: pk.fingerprint(HashAlg::Sha256).to_string(),
            changed,
            changed_line,
        };
        self.app.emit("hostkey-prompt", prompt)?;
        let accepted = tokio::time::timeout(Duration::from_secs(180), rx).await.ok().and_then(|r| r.ok()).unwrap_or(false);
        self.pending.lock().remove(&request_id);
        if accepted && !changed {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = keys::known_hosts::learn_known_hosts_path(&self.host, self.port, &pk, &path) {
                log::warn!("could not write known_hosts: {e}");
            }
        }
        Ok(accepted)
    }
}

pub struct SshConn {
    pub handle: Handle<Client>,
    pub host: Host,
    /// Keeps the jump connection alive for as long as this one lives.
    _jump: Option<Arc<SshConn>>,
}

impl SshConn {
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Run a command and collect stdout (stderr appended on failure).
    pub async fn exec_capture(&self, cmd: &str) -> Result<String> {
        let mut ch = self.handle.channel_open_session().await?;
        ch.exec(true, cmd).await?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut code = None;
        while let Some(msg) = ch.wait().await {
            match msg {
                russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
                russh::ChannelMsg::ExtendedData { data, .. } => err.extend_from_slice(&data),
                russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        if code.unwrap_or(0) != 0 {
            bail!("`{cmd}` failed ({}): {}", code.unwrap_or(0), String::from_utf8_lossy(&err).trim());
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

/// Credentials pulled out of the vault before connecting so the vault lock
/// is never held across awaits.
enum Creds {
    Password(Zeroizing<String>),
    Key(Arc<keys::PrivateKey>),
    Agent,
}

fn resolve_creds(state: &AppState, host: &Host) -> Result<Creds> {
    let vault = state.vault.lock();
    let need_vault = || anyhow!("unlock the vault to use stored credentials for {}", host.name);
    Ok(match &host.auth {
        AuthMethod::Password { secret_id } => {
            let id = secret_id.as_deref().ok_or_else(|| anyhow!("no password secret selected for {}", host.name))?;
            if !vault.is_unlocked() {
                return Err(need_vault());
            }
            Creds::Password(Zeroizing::new(vault.get(id)?.value.clone()))
        }
        AuthMethod::VaultKey { secret_id } => {
            let id = secret_id.as_deref().ok_or_else(|| anyhow!("no key secret selected for {}", host.name))?;
            if !vault.is_unlocked() {
                return Err(need_vault());
            }
            let s = vault.get(id)?;
            if s.kind != SecretKind::PrivateKey {
                bail!("secret '{}' is not a private key", s.name);
            }
            let k = keys::decode_secret_key(&s.value, s.passphrase.as_deref()).context("decoding private key")?;
            Creds::Key(Arc::new(k))
        }
        AuthMethod::KeyFile { path, passphrase_secret_id } => {
            let pass = match passphrase_secret_id {
                Some(id) => {
                    if !vault.is_unlocked() {
                        return Err(need_vault());
                    }
                    Some(Zeroizing::new(vault.get(id)?.value.clone()))
                }
                None => None,
            };
            let p = crate::paths::expand_tilde(path);
            let k = keys::load_secret_key(&p, pass.as_deref().map(|s| s.as_str()))
                .with_context(|| format!("loading key {}", p.display()))?;
            Creds::Key(Arc::new(k))
        }
        AuthMethod::Agent => Creds::Agent,
    })
}

async fn authenticate(handle: &mut Handle<Client>, user: &str, creds: Creds) -> Result<()> {
    match creds {
        Creds::Password(pw) => {
            if handle.authenticate_password(user, pw.as_str()).await?.success() {
                return Ok(());
            }
            // Many servers only allow keyboard-interactive; answer every prompt with the password.
            let mut resp = handle.authenticate_keyboard_interactive_start(user, None).await?;
            for _ in 0..5 {
                match resp {
                    KeyboardInteractiveAuthResponse::Success => return Ok(()),
                    KeyboardInteractiveAuthResponse::Failure { .. } => break,
                    KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        let answers = prompts.iter().map(|_| pw.to_string()).collect();
                        resp = handle.authenticate_keyboard_interactive_respond(answers).await?;
                    }
                }
            }
            bail!("password authentication failed for {user}")
        }
        Creds::Key(key) => {
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            if handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(key, hash)).await?.success() {
                Ok(())
            } else {
                bail!("public key authentication failed for {user}")
            }
        }
        Creds::Agent => {
            let mut agent = keys::agent::client::AgentClient::connect_env()
                .await
                .context("connecting to ssh-agent (is SSH_AUTH_SOCK set?)")?;
            let ids = agent.request_identities().await?;
            if ids.is_empty() {
                bail!("ssh-agent has no identities loaded");
            }
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            for id in ids {
                let pk = id.public_key().into_owned();
                match handle.authenticate_publickey_with(user, pk, hash, &mut agent).await {
                    Ok(r) if r.success() => return Ok(()),
                    _ => continue,
                }
            }
            bail!("no ssh-agent identity was accepted for {user}")
        }
    }
}

fn client_config(host: &Host) -> Arc<client::Config> {
    Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(host.keepalive_secs.unwrap_or(30).max(5))),
        keepalive_max: 4,
        inactivity_timeout: None,
        nodelay: true,
        ..Default::default()
    })
}

/// Open a fresh connection to `host_id` (recursively through jump hosts).
pub fn connect(app: AppHandle, host_id: String, depth: u8) -> Pin<Box<dyn Future<Output = Result<Arc<SshConn>>> + Send>> {
    Box::pin(async move {
        if depth > 5 {
            bail!("ProxyJump chain is too deep (loop?)");
        }
        let state = app.state::<AppState>();
        let host = state.hosts.lock().get(&host_id)?;
        let creds = resolve_creds(&state, &host)?;
        let handler = Client {
            app: app.clone(),
            host: host.hostname.clone(),
            port: host.port,
            pending: state.hostkey_pending.clone(),
        };
        let config = client_config(&host);
        let (mut handle, jump) = match &host.jump_host_id {
            Some(jid) => {
                let jump = get_conn(app.clone(), jid.clone(), depth + 1).await?;
                let ch = jump
                    .handle
                    .channel_open_direct_tcpip(host.hostname.clone(), host.port as u32, "127.0.0.1", 0)
                    .await
                    .with_context(|| format!("opening tunnel to {} via {}", host.hostname, jump.host.name))?;
                let h = client::connect_stream(config, ch.into_stream(), handler).await?;
                (h, Some(jump))
            }
            None => {
                let addr = (host.hostname.as_str(), host.port);
                let h = tokio::time::timeout(Duration::from_secs(20), client::connect(config, addr, handler))
                    .await
                    .map_err(|_| anyhow!("timed out connecting to {}:{}", host.hostname, host.port))??;
                (h, None)
            }
        };
        authenticate(&mut handle, &host.username, creds).await?;
        Ok(Arc::new(SshConn { handle, host, _jump: jump }))
    })
}

/// Get a pooled connection for the host, connecting if needed.
pub async fn get_conn(app: AppHandle, host_id: String, depth: u8) -> Result<Arc<SshConn>> {
    {
        let state = app.state::<AppState>();
        let conns = state.conns.lock().await;
        if let Some(c) = conns.get(&host_id) {
            if !c.is_closed() {
                return Ok(c.clone());
            }
        }
    }
    let conn = connect(app.clone(), host_id.clone(), depth).await?;
    let state = app.state::<AppState>();
    state.conns.lock().await.insert(host_id, conn.clone());
    Ok(conn)
}

pub async fn disconnect(state: &AppState, host_id: &str) {
    state.sftp.lock().await.remove(host_id);
    state.dockers.lock().await.remove(host_id);
    if let Some(c) = state.conns.lock().await.remove(host_id) {
        let _ = c.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    }
}

/// Local port forward: 127.0.0.1:local_port → remote_host:remote_port via SSH.
pub async fn start_forward(app: AppHandle, host_id: String, fwd: PortForward) -> Result<String> {
    let conn = get_conn(app.clone(), host_id.clone(), 0).await?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", fwd.local_port))
        .await
        .with_context(|| format!("binding 127.0.0.1:{}", fwd.local_port))?;
    let task_id = format!("fwd:{host_id}:{}", fwd.id);
    let rhost = fwd.remote_host.clone();
    let rport = fwd.remote_port as u32;
    let h = tokio::spawn(async move {
        loop {
            let Ok((mut sock, peer)) = listener.accept().await else { break };
            let conn = conn.clone();
            let rhost = rhost.clone();
            tokio::spawn(async move {
                let ch = match conn
                    .handle
                    .channel_open_direct_tcpip(rhost, rport, peer.ip().to_string(), peer.port() as u32)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("forward open failed: {e}");
                        return;
                    }
                };
                let mut stream = ch.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut sock, &mut stream).await;
            });
        }
    });
    app.state::<AppState>().add_task(task_id.clone(), h.abort_handle());
    Ok(task_id)
}
