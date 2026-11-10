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
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<HostKeyDecision>>>>,
}

/// User's answer to a host key prompt.
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyDecision {
    Reject,
    /// Connect this time without touching known_hosts.
    Once,
    /// Save to known_hosts (replacing the old key when it changed).
    Trust,
}

/// `[host]:port` form used by known_hosts for non-default ports.
fn known_hosts_name(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Remove existing entries for the host via `ssh-keygen -R` (handles hashed
/// entries and leaves a `known_hosts.old` backup).
fn forget_known_host(host: &str, port: u16, path: &std::path::Path) -> Result<()> {
    let out = std::process::Command::new("ssh-keygen")
        .arg("-R")
        .arg(known_hosts_name(host, port))
        .arg("-f")
        .arg(path)
        .output()
        .context("running ssh-keygen -R")?;
    if !out.status.success() {
        bail!("ssh-keygen -R failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
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

struct PromptGuard {
    app: AppHandle,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<HostKeyDecision>>>>,
    request_id: String,
}

impl Drop for PromptGuard {
    fn drop(&mut self) {
        self.pending.lock().remove(&self.request_id);
        let _ = self.app.emit("hostkey-prompt-done", self.request_id.clone());
    }
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let pk = key.public_key();
        let path = known_hosts_file();
        let (changed, changed_line) = match crate::known_hosts::check(&self.host, self.port, &pk, &path) {
            crate::known_hosts::Verdict::Known => return Ok(true),
            crate::known_hosts::Verdict::Unknown => (false, None),
            crate::known_hosts::Verdict::Changed { line } => (true, line),
            crate::known_hosts::Verdict::Revoked { line } => {
                let _ = self.app.emit(
                    "app-error",
                    format!("{}:{} presented a host key marked @revoked in known_hosts (line {line}). Connection refused.", self.host, self.port),
                );
                return Ok(false);
            }
        };
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(request_id.clone(), tx);
        // However this prompt ends (answer, timeout, or the connection attempt
        // being dropped), remove it and close its dialog in the UI.
        let _guard = PromptGuard { app: self.app.clone(), pending: self.pending.clone(), request_id: request_id.clone() };
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
        let decision = tokio::time::timeout(Duration::from_secs(180), rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(HostKeyDecision::Reject);
        self.pending.lock().remove(&request_id);
        // A changed key is either replaced after confirmation or rejected:
        // never "connect once", which would send credentials to a possible MITM.
        let decision = if changed && decision == HostKeyDecision::Once { HostKeyDecision::Reject } else { decision };
        if decision == HostKeyDecision::Trust {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if changed {
                if let Err(e) = forget_known_host(&self.host, self.port, &path) {
                    log::warn!("could not remove old host key: {e:#}");
                    let _ = self.app.emit("app-error", format!("Could not remove the old host key: {e:#}"));
                    return Ok(false);
                }
            }
            if let Err(e) = keys::known_hosts::learn_known_hosts_path(&self.host, self.port, &pk, &path) {
                log::warn!("could not write known_hosts: {e}");
            }
        }
        Ok(decision != HostKeyDecision::Reject)
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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthPrompt {
    pub request_id: String,
    pub host: String,
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<AuthPromptField>,
}

#[derive(Serialize, Clone)]
pub struct AuthPromptField {
    pub prompt: String,
    pub echo: bool,
}

fn is_password_prompt(p: &str) -> bool {
    let p = p.to_lowercase();
    (p.contains("password") || p.contains("passphrase")) && !p.contains("code") && !p.contains("otp") && !p.contains("token")
}

/// Ask the UI to answer keyboard-interactive prompts (OTP, 2FA codes, …).
async fn ask_user(app: &AppHandle, host: &str, name: String, instructions: String, prompts: &[russh::client::Prompt]) -> Result<Vec<String>> {
    let state = app.state::<AppState>();
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    state.auth_pending.lock().insert(request_id.clone(), tx);
    app.emit(
        "auth-prompt",
        AuthPrompt {
            request_id: request_id.clone(),
            host: host.to_string(),
            name,
            instructions,
            prompts: prompts.iter().map(|p| AuthPromptField { prompt: p.prompt.clone(), echo: p.echo }).collect(),
        },
    )?;
    let answer = tokio::time::timeout(Duration::from_secs(300), rx).await.ok().and_then(|r| r.ok()).flatten();
    state.auth_pending.lock().remove(&request_id);
    answer.ok_or_else(|| anyhow!("authentication cancelled"))
}

/// Run keyboard-interactive auth. Password-looking prompts are answered from
/// `password` (once); anything else (verification codes) is asked of the user.
async fn keyboard_interactive(app: &AppHandle, handle: &mut Handle<Client>, host: &str, user: &str, password: Option<&str>) -> Result<bool> {
    let mut resp = handle.authenticate_keyboard_interactive_start(user, None).await?;
    let mut password_used = false;
    for _ in 0..10 {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                let answers = if prompts.is_empty() {
                    vec![]
                } else if let (Some(pw), false) = (password, password_used) {
                    if prompts.iter().all(|p| is_password_prompt(&p.prompt)) {
                        password_used = true;
                        prompts.iter().map(|_| pw.to_string()).collect()
                    } else {
                        ask_user(app, host, name, instructions, &prompts).await?
                    }
                } else {
                    ask_user(app, host, name, instructions, &prompts).await?
                };
                resp = handle.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Ok(false)
}

fn partial_success(r: &russh::client::AuthResult) -> bool {
    matches!(r, russh::client::AuthResult::Failure { partial_success: true, .. })
}

async fn authenticate(app: &AppHandle, handle: &mut Handle<Client>, host: &str, user: &str, creds: Creds) -> Result<()> {
    match creds {
        Creds::Password(pw) => {
            let r = handle.authenticate_password(user, pw.as_str()).await?;
            if r.success() {
                return Ok(());
            }
            // Servers that only allow keyboard-interactive, or want a second factor.
            if keyboard_interactive(app, handle, host, user, Some(pw.as_str())).await? {
                return Ok(());
            }
            bail!("password authentication failed for {user}")
        }
        Creds::Key(key) => {
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let r = handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(key, hash)).await?;
            if r.success() {
                return Ok(());
            }
            if partial_success(&r) && keyboard_interactive(app, handle, host, user, None).await? {
                return Ok(());
            }
            bail!("public key authentication failed for {user}")
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
                    Ok(r) if partial_success(&r) => {
                        if keyboard_interactive(app, handle, host, user, None).await? {
                            return Ok(());
                        }
                        break;
                    }
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
                let tcp = tokio::time::timeout(Duration::from_secs(20), tokio::net::TcpStream::connect(addr))
                    .await
                    .map_err(|_| anyhow!("timed out connecting to {}:{}", host.hostname, host.port))?
                    .with_context(|| format!("connecting to {}:{}", host.hostname, host.port))?;
                let _ = tcp.set_nodelay(true);
                // The handshake may wait on the host-key dialog (up to 180 s), so
                // its limit must be longer than that.
                let h = tokio::time::timeout(Duration::from_secs(200), client::connect_stream(config, tcp, handler))
                    .await
                    .map_err(|_| anyhow!("SSH handshake with {}:{} timed out", host.hostname, host.port))??;
                (h, None)
            }
        };
        authenticate(&app, &mut handle, &host.name, &host.username, creds).await?;
        Ok(Arc::new(SshConn { handle, host, _jump: jump }))
    })
}

/// Get a pooled connection for the host, connecting if needed.
pub fn get_conn(app: AppHandle, host_id: String, depth: u8) -> Pin<Box<dyn Future<Output = Result<Arc<SshConn>>> + Send>> {
    Box::pin(get_conn_inner(app, host_id, depth))
}

async fn get_conn_inner(app: AppHandle, host_id: String, depth: u8) -> Result<Arc<SshConn>> {
    // Serialise connection attempts per host, so a terminal and an SFTP pane
    // opening together share one connection (and one host-key prompt).
    let gate = app.state::<AppState>().connecting.lock().entry(host_id.clone()).or_default().clone();
    let _connecting = gate.lock().await;
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
    state.conns.lock().await.insert(host_id.clone(), conn.clone());
    // Start forwards marked auto-start the first time the host connects.
    for f in conn.host.forwards.iter().filter(|f| f.auto_start) {
        if !forward_active(&state, &host_id, &f.id) {
            let fut: Pin<Box<dyn Future<Output = Result<String>> + Send>> = Box::pin(start_forward(app.clone(), host_id.clone(), f.clone()));
            let app2 = app.clone();
            let name = format!("{}:{}", conn.host.name, f.local_port);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    let _ = app2.emit("app-error", format!("Auto-start forward {name} failed: {e:#}"));
                }
            });
        }
    }
    Ok(conn)
}

pub fn forward_task_id(host_id: &str, forward_id: &str) -> String {
    format!("fwd:{host_id}:{forward_id}")
}

pub fn forward_active(state: &AppState, host_id: &str, forward_id: &str) -> bool {
    state.tasks.lock().get(&forward_task_id(host_id, forward_id)).is_some_and(|h| !h.is_finished())
}

/// (host id, forward id) of every running forward.
pub fn active_forwards(state: &AppState) -> Vec<(String, String)> {
    state
        .tasks
        .lock()
        .iter()
        .filter(|(k, h)| k.starts_with("fwd:") && !h.is_finished())
        .filter_map(|(k, _)| {
            let mut parts = k.splitn(3, ':').skip(1);
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
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
    let task_id = forward_task_id(&host_id, &fwd.id);
    if forward_active(&app.state::<AppState>(), &host_id, &fwd.id) {
        return Ok(task_id);
    }
    // Connect first so auth problems surface now rather than on first use.
    let first = get_conn(app.clone(), host_id.clone(), 0).await?;
    drop(first);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", fwd.local_port))
        .await
        .with_context(|| format!("port {} is not available on 127.0.0.1", fwd.local_port))?;
    let rhost = fwd.remote_host.clone();
    let rport = fwd.remote_port as u32;
    let app2 = app.clone();
    let host2 = host_id.clone();
    let h = tokio::spawn(async move {
        loop {
            let Ok((mut sock, peer)) = listener.accept().await else { break };
            let rhost = rhost.clone();
            let app = app2.clone();
            let host_id = host2.clone();
            tokio::spawn(async move {
                // Pooled: reconnects transparently if the SSH link dropped.
                let conn = match get_conn(app.clone(), host_id, 0).await {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("forward reconnect failed: {e:#}");
                        return;
                    }
                };
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
    let _ = app.emit("forwards-changed", ());
    Ok(task_id)
}

pub fn stop_forward(app: &AppHandle, host_id: &str, forward_id: &str) -> bool {
    let stopped = app.state::<AppState>().cancel_task(&forward_task_id(host_id, forward_id));
    let _ = app.emit("forwards-changed", ());
    stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forget_known_host_removes_only_that_host() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let key = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
        std::fs::write(
            &path,
            format!("# comment\n[127.0.0.1]:2222 ssh-ed25519 {key}\nexample.com ssh-ed25519 {key}\n"),
        )
        .unwrap();
        forget_known_host("127.0.0.1", 2222, &path).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("[127.0.0.1]:2222"), "{after}");
        assert!(after.contains("example.com"));
        assert!(dir.path().join("known_hosts.old").exists());
    }

    #[test]
    fn password_prompt_detection() {
        assert!(is_password_prompt("Password: "));
        assert!(is_password_prompt("test@host's password:"));
        assert!(!is_password_prompt("Verification code: "));
        assert!(!is_password_prompt("One-time password (OTP): "));
    }
}
