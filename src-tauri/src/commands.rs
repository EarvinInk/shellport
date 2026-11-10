//! Thin #[tauri::command] wrappers.

use crate::docker::{self, Action, ContainerInfo, EndpointInfo, ImageInfo};
use crate::error::CmdResult;
use crate::sftp::{self, FileEntry, TransferEvent};
use crate::state::AppState;
use crate::store::databases::{DbProfile, Safety};
use crate::store::hosts::Host;
use crate::store::settings::Settings;
use crate::store::ssh_config::{self, SshConfigEntry};
use crate::term::{self, TermEvent};
use crate::vault::{self, SecretInput, SecretMeta, VaultStatus};
use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

// ---------- vault ----------

fn status(state: &AppState) -> VaultStatus {
    let v = state.vault.lock();
    VaultStatus { exists: v.exists(), unlocked: v.is_unlocked(), remembered: vault::keyring::is_stored() }
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> VaultStatus {
    status(&state)
}

#[tauri::command]
pub async fn vault_create(state: State<'_, AppState>, password: String, remember: bool) -> CmdResult<VaultStatus> {
    {
        let mut v = state.vault.lock();
        v.create(&password, vault::KdfParams::default())?;
    }
    if remember {
        let pw = password.clone();
        let mut tmp = vault::Vault::new(state.vault.lock().path().to_path_buf());
        let key = tokio::task::spawn_blocking(move || tmp.unlock(&pw)).await??;
        vault::keyring::store(&key)?;
    }
    Ok(status(&state))
}

#[tauri::command]
pub async fn vault_unlock(state: State<'_, AppState>, password: String, remember: bool) -> CmdResult<VaultStatus> {
    // Argon2 is deliberately slow; derive the key off the async runtime.
    let path = state.vault.lock().path().to_path_buf();
    let (tmp, key) = tokio::task::spawn_blocking(move || {
        let mut tmp = vault::Vault::new(path);
        let key = tmp.unlock(&password)?;
        anyhow::Ok((tmp, key))
    })
    .await??;
    drop(tmp);
    state.vault.lock().unlock_with_key(key.clone())?;
    if remember {
        vault::keyring::store(&key)?;
    }
    Ok(status(&state))
}

/// Try to unlock with the key remembered in the system keyring.
#[tauri::command]
pub async fn vault_unlock_keyring(state: State<'_, AppState>) -> CmdResult<VaultStatus> {
    let key = tokio::task::spawn_blocking(vault::keyring::load).await??;
    match key {
        Some(k) => {
            if let Err(e) = state.vault.lock().unlock_with_key(k) {
                // Stale entry (e.g. password changed elsewhere): forget it.
                let _ = vault::keyring::clear();
                return Err(e.into());
            }
        }
        None => return Err(anyhow!("no key remembered in the system keyring").into()),
    }
    Ok(status(&state))
}

#[tauri::command]
pub fn vault_lock(state: State<'_, AppState>) -> VaultStatus {
    state.vault.lock().lock();
    status(&state)
}

#[tauri::command]
pub fn vault_forget_device(state: State<'_, AppState>) -> CmdResult<VaultStatus> {
    vault::keyring::clear()?;
    Ok(status(&state))
}

#[tauri::command]
pub async fn vault_change_password(state: State<'_, AppState>, old: String, new: String) -> CmdResult<()> {
    let key = state.vault.lock().change_password(&old, &new)?;
    if vault::keyring::is_stored() {
        vault::keyring::store(&key)?;
    }
    Ok(())
}

#[tauri::command]
pub fn secrets_list(state: State<'_, AppState>) -> CmdResult<Vec<SecretMeta>> {
    let mut v = state.vault.lock();
    v.touch();
    Ok(v.list()?)
}

#[tauri::command]
pub fn secret_save(state: State<'_, AppState>, input: SecretInput) -> CmdResult<SecretMeta> {
    Ok(state.vault.lock().upsert(input)?)
}

#[tauri::command]
pub fn secret_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.vault.lock().delete(&id)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revealed {
    value: zeroize::Zeroizing<String>,
    passphrase: Option<zeroize::Zeroizing<String>>,
}

/// At most this many reveals per minute: a person clicking "show"/"copy"
/// never gets near it, a script dumping the vault does.
const REVEALS_PER_MINUTE: usize = 20;

/// Explicit user action: show/copy a secret value.
#[tauri::command]
pub fn secret_reveal(state: State<'_, AppState>, id: String) -> CmdResult<Revealed> {
    {
        let now = std::time::Instant::now();
        let mut r = state.reveals.lock();
        while r.front().is_some_and(|t| now.duration_since(*t) > std::time::Duration::from_secs(60)) {
            r.pop_front();
        }
        if r.len() >= REVEALS_PER_MINUTE {
            return Err(anyhow!("too many secrets revealed in the last minute; try again shortly").into());
        }
        r.push_back(now);
    }
    let mut v = state.vault.lock();
    v.touch();
    let s = v.get(&id)?;
    Ok(Revealed {
        value: zeroize::Zeroizing::new(s.value.clone()),
        passphrase: s.passphrase.clone().map(zeroize::Zeroizing::new),
    })
}

#[tauri::command]
pub fn secret_public_key(state: State<'_, AppState>, id: String) -> CmdResult<String> {
    Ok(state.vault.lock().public_key_openssh(&id)?)
}

/// Generate a new ed25519 key pair and store it in the vault.
#[tauri::command]
pub fn secret_generate_key(state: State<'_, AppState>, name: String) -> CmdResult<SecretMeta> {
    let mut seed = zeroize::Zeroizing::new([0u8; 32]);
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, seed.as_mut());
    let mut key = russh::keys::PrivateKey::from(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&seed));
    key.set_comment(format!("shellport:{name}"));
    let pem = key.to_openssh(russh::keys::ssh_key::LineEnding::LF).map_err(|e| anyhow!("{e}"))?;
    Ok(state.vault.lock().upsert(SecretInput {
        id: None,
        name,
        kind: vault::SecretKind::PrivateKey,
        username: None,
        value: Some(pem.to_string()),
        passphrase: None,
        notes: "Generated by Shellport (ed25519)".into(),
    })?)
}

// ---------- hosts ----------

/// Hosts, with those linked to `~/.ssh/config` refreshed from the file.
#[tauri::command]
pub fn hosts_list(state: State<'_, AppState>) -> Vec<Host> {
    let mut store = state.hosts.lock();
    let hosts = store.list();
    if hosts.iter().any(|h| h.ssh_config_alias.is_some()) {
        let entries = ssh_config::parse_file(&ssh_config::default_path());
        let changed = ssh_config::refresh_linked(&hosts, &entries);
        if !changed.is_empty() {
            if let Err(e) = store.replace_all(changed) {
                log::warn!("saving hosts refreshed from ~/.ssh/config: {e:#}");
            }
            return store.list();
        }
    }
    hosts
}

#[tauri::command]
pub fn host_save(state: State<'_, AppState>, host: Host) -> CmdResult<Host> {
    let mut store = state.hosts.lock();
    let saved = store.upsert(host)?;
    // Linked hosts are written back to ~/.ssh/config, keeping the rest of the file.
    if saved.ssh_config_alias.is_some() {
        let jump_alias = saved.jump_host_id.as_deref().and_then(|j| store.get(j).ok()).and_then(|j| j.ssh_config_alias);
        ssh_config::write_linked(&ssh_config::default_path(), &saved, jump_alias.as_deref())
            .map_err(|e| anyhow!("saved in Shellport, but ~/.ssh/config could not be updated: {e:#}"))?;
    }
    Ok(saved)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    id: String,
    /// None: not checked (reached through a jump host or proxy).
    up: Option<bool>,
}

/// Can each host be reached? A TCP connect only (no login); hosts behind a
/// jump host report their jump host's state.
#[tauri::command]
pub async fn hosts_status(state: State<'_, AppState>) -> CmdResult<Vec<HostStatus>> {
    let hosts = state.hosts.lock().list();
    let direct = futures_util::future::join_all(hosts.iter().map(|h| {
        let (addr, port) = (h.hostname.clone(), h.port);
        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let Ok(Ok(mut s)) = tokio::time::timeout(std::time::Duration::from_secs(3), tokio::net::TcpStream::connect((addr.as_str(), port))).await
            else {
                return false;
            };
            // Greet like `ssh-keyscan` and read the server's banner, so the
            // server logs an ordinary short connection rather than a stall.
            let _ = s.write_all(b"SSH-2.0-Shellport_status\r\n").await;
            let mut buf = [0u8; 64];
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), s.read(&mut buf)).await;
            true
        }
    }))
    .await;
    let by_id: std::collections::HashMap<&str, bool> = hosts.iter().zip(&direct).map(|(h, up)| (h.id.as_str(), *up)).collect();
    Ok(hosts
        .iter()
        .zip(&direct)
        .map(|(h, up)| HostStatus {
            id: h.id.clone(),
            up: match &h.jump_host_id {
                None => Some(*up),
                Some(j) => by_id.get(j.as_str()).copied().filter(|j_up| !j_up).map(|_| false),
            },
        })
        .collect())
}

/// Fill a database connection from a `postgres://` URL.
#[tauri::command]
pub fn db_parse_url(url: String) -> CmdResult<crate::store::address::DbUrl> {
    Ok(crate::store::address::parse_db_url(&url)?)
}

/// Export hosts (no secrets) as JSON or CSV to a file the user picks.
#[tauri::command]
pub async fn hosts_export(app: AppHandle, state: State<'_, AppState>, format: String) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let hosts = state.hosts.lock().list();
    let (text, ext) = match format.as_str() {
        "csv" => (crate::store::host_io::export_csv(&hosts), "csv"),
        _ => (crate::store::host_io::export_json(&hosts)?, "json"),
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export hosts")
        .set_file_name(format!("shellport-hosts.{ext}"))
        .add_filter(if ext == "csv" { "CSV" } else { "JSON" }, &[ext])
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(picked) = rx.await.ok().flatten() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| anyhow!("unsupported file location: {e}"))?;
    crate::paths::write_private(&path, text.as_bytes())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostImportPreview {
    names: Vec<String>,
    /// Names already present here (skipped on import).
    existing: Vec<String>,
}

fn read_host_file(state: &AppState, path: &str) -> anyhow::Result<crate::store::host_io::Imported> {
    let p = granted_path(state, std::path::Path::new(path), false)?;
    if std::fs::metadata(&p)?.len() > 5 * 1024 * 1024 {
        anyhow::bail!("the file is too large for a hosts list");
    }
    crate::store::host_io::parse(&std::fs::read_to_string(&p)?)
}

#[tauri::command]
pub fn hosts_import_preview(state: State<'_, AppState>, path: String) -> CmdResult<HostImportPreview> {
    let imp = read_host_file(&state, &path)?;
    let have: Vec<String> = state.hosts.lock().list().into_iter().map(|h| h.name).collect();
    let names: Vec<String> = imp.hosts.iter().map(|h| h.name.clone()).collect();
    let existing = names.iter().filter(|n| have.contains(n)).cloned().collect();
    Ok(HostImportPreview { names, existing })
}

/// Import hosts from a JSON or CSV file, skipping names that already exist and
/// linking jump hosts by name.
#[tauri::command]
pub fn hosts_import_file(state: State<'_, AppState>, path: String) -> CmdResult<usize> {
    let imp = read_host_file(&state, &path)?;
    let mut store = state.hosts.lock();
    let jumps: Vec<(String, String)> = imp.hosts.iter().zip(&imp.jumps).filter_map(|(h, j)| j.clone().map(|j| (h.name.clone(), j))).collect();
    let n = store.import(imp.hosts)?;
    let all = store.list();
    let id_of = |name: &str| all.iter().find(|h| h.name == name).map(|h| h.id.clone());
    let mut changed = Vec::new();
    for (name, jump) in jumps {
        if let (Some(mut h), Some(j)) = (all.iter().find(|h| h.name == name).cloned(), id_of(&jump)) {
            if h.jump_host_id.is_none() && h.id != j {
                h.jump_host_id = Some(j);
                changed.push(h);
            }
        }
    }
    store.replace_all(changed)?;
    granted_path(&state, std::path::Path::new(&path), true).ok();
    Ok(n)
}

#[tauri::command]
pub async fn host_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    crate::ssh::disconnect(&state, &id).await;
    state.hosts.lock().delete(&id)?;
    Ok(state.dbs.lock().host_deleted(&id)?)
}

/// Resolve an ssh_config path: the default `~/.ssh/config`, or a file the
/// user picked with `local_pick_file`.
fn ssh_config_path(state: &AppState, path: Option<String>) -> anyhow::Result<std::path::PathBuf> {
    let default = ssh_config::default_path();
    match path {
        None => Ok(default),
        Some(p) => {
            let p = crate::paths::expand_tilde(&p);
            let same_as_default = matches!((std::fs::canonicalize(&p), std::fs::canonicalize(&default)), (Ok(a), Ok(b)) if a == b);
            if same_as_default || p == default {
                Ok(default)
            } else {
                granted_path(state, &p, false)
            }
        }
    }
}

#[tauri::command]
pub fn ssh_config_preview(state: State<'_, AppState>, path: Option<String>) -> CmdResult<Vec<SshConfigEntry>> {
    Ok(ssh_config::parse_file(&ssh_config_path(&state, path)?))
}

#[tauri::command]
pub fn ssh_config_import(state: State<'_, AppState>, path: Option<String>, aliases: Vec<String>) -> CmdResult<usize> {
    let p = ssh_config_path(&state, path)?;
    let entries = ssh_config::parse_file(&p);
    // Hosts imported from ~/.ssh/config stay linked to it; from another file
    // they are a one-time copy.
    let linked = p == ssh_config::default_path();
    let hosts: Vec<Host> = ssh_config::to_hosts(&entries)
        .into_iter()
        .filter(|h| aliases.contains(&h.name))
        .map(|h| Host { ssh_config_alias: h.ssh_config_alias.filter(|_| linked), ..h })
        .collect();
    Ok(state.hosts.lock().import(hosts)?)
}

#[tauri::command]
pub async fn host_disconnect(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    crate::ssh::disconnect(&state, &id).await;
    Ok(())
}

#[tauri::command]
pub async fn host_test(app: AppHandle, id: String) -> CmdResult<String> {
    let conn = crate::ssh::connect(app, id, 0).await?;
    let out = conn.exec_capture("uname -srm; uptime").await.unwrap_or_default();
    let _ = conn.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    Ok(out.trim().to_string())
}

#[tauri::command]
pub fn hostkey_respond(state: State<'_, AppState>, request_id: String, decision: crate::ssh::HostKeyDecision) {
    if let Some(tx) = state.hostkey_pending.lock().remove(&request_id) {
        let _ = tx.send(decision);
    }
}

/// Answers for a keyboard-interactive (2FA) prompt; `None` cancels the login.
#[tauri::command]
pub fn auth_prompt_respond(state: State<'_, AppState>, request_id: String, answers: Option<Vec<String>>) {
    if let Some(tx) = state.auth_pending.lock().remove(&request_id) {
        let _ = tx.send(answers);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveForward {
    host_id: String,
    forward_id: String,
}

#[tauri::command]
pub fn forwards_active(state: State<'_, AppState>) -> Vec<ActiveForward> {
    crate::ssh::active_forwards(&state)
        .into_iter()
        .map(|(host_id, forward_id)| ActiveForward { host_id, forward_id })
        .collect()
}

#[tauri::command]
pub fn forward_stop(app: AppHandle, host_id: String, forward_id: String) -> bool {
    crate::ssh::stop_forward(&app, &host_id, &forward_id)
}

#[tauri::command]
pub async fn forward_start(app: AppHandle, state: State<'_, AppState>, host_id: String, forward_id: String) -> CmdResult<String> {
    let host = state.hosts.lock().get(&host_id)?;
    let fwd = host.forwards.into_iter().find(|f| f.id == forward_id).ok_or_else(|| anyhow!("forward not found"))?;
    Ok(crate::ssh::start_forward(app, host_id, fwd).await?)
}

#[tauri::command]
pub fn task_cancel(state: State<'_, AppState>, task_id: String) -> bool {
    state.cancel_task(&task_id)
}

#[tauri::command]
pub fn tasks_active(state: State<'_, AppState>) -> Vec<String> {
    state.tasks.lock().iter().filter(|(_, h)| !h.is_finished()).map(|(k, _)| k.clone()).collect()
}

// ---------- databases ----------

#[tauri::command]
pub fn db_engines() -> Vec<crate::db::EngineInfo> {
    crate::db::registry::engines()
}

#[tauri::command]
pub fn dbs_list(state: State<'_, AppState>) -> Vec<DbProfile> {
    state.dbs.lock().list()
}

#[tauri::command]
pub fn db_save(state: State<'_, AppState>, db: DbProfile) -> CmdResult<DbProfile> {
    Ok(state.dbs.lock().upsert(db)?)
}

#[tauri::command]
pub async fn db_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let open: Vec<String> = state.db_sessions.lock().await.iter().filter(|(_, o)| o.profile_id == id).map(|(k, _)| k.clone()).collect();
    for sid in open {
        crate::db::session::close(&state, &sid).await;
    }
    Ok(state.dbs.lock().delete(&id)?)
}

/// Connect with a (possibly unsaved) profile, report the server version, disconnect.
#[tauri::command]
pub async fn db_test(app: AppHandle, state: State<'_, AppState>, db: DbProfile) -> CmdResult<String> {
    let sid = format!("test-{}", uuid::Uuid::new_v4());
    let (session, tunnel) = crate::db::session::open(&app, &db, &sid).await?;
    let version = session.server_version();
    session.close().await;
    if let Some(t) = tunnel {
        state.cancel_task(&t);
    }
    Ok(version)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbConnected {
    session_id: String,
    server_version: String,
    engine: crate::db::DbEngine,
    safety: Safety,
    production: bool,
}

#[tauri::command]
pub async fn db_connect(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<DbConnected> {
    let db = state.dbs.lock().get(&id)?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let host_production = db.via_host_id.as_deref().and_then(|h| state.hosts.lock().get(h).ok()).is_some_and(|h| h.production);
    let safety = db.effective_safety(host_production);
    let production = db.production || host_production;
    let (session, tunnel_task) = crate::db::session::open(&app, &db, &session_id).await?;
    let server_version = session.server_version();
    state
        .db_sessions
        .lock()
        .await
        .insert(session_id.clone(), crate::db::session::OpenDb { session, profile_id: id, tunnel_task, safety });
    Ok(DbConnected { session_id, server_version, engine: db.engine, safety, production })
}

#[tauri::command]
pub async fn db_disconnect(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    crate::db::session::close(&state, &session_id).await;
    Ok(())
}

#[tauri::command]
pub async fn db_children(state: State<'_, AppState>, session_id: String, path: Vec<String>) -> CmdResult<Vec<crate::db::SchemaNode>> {
    let (s, _) = crate::db::session::get(&state, &session_id).await?;
    Ok(s.children(&path).await.map_err(|e| crate::db::session::map_lost(s.as_ref(), e))?)
}

/// Upper bound on rows kept per statement, whatever the UI asks for.
const DB_MAX_ROWS: usize = 50_000;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QueryOutcome {
    Done { output: crate::db::QueryOutput },
    /// Nothing ran: these statements need the user's go-ahead first.
    NeedsConfirmation { risks: Vec<crate::db::risk::Risk> },
}

#[tauri::command]
pub async fn db_query(state: State<'_, AppState>, session_id: String, sql: String, row_limit: usize, confirmed: bool) -> CmdResult<QueryOutcome> {
    let (s, safety) = crate::db::session::get(&state, &session_id).await?;
    if safety == Safety::Confirm && !confirmed {
        let risks = crate::db::risk::classify(&sql);
        if !risks.is_empty() {
            return Ok(QueryOutcome::NeedsConfirmation { risks });
        }
    }
    let output = s.query(&sql, row_limit.clamp(1, DB_MAX_ROWS)).await.map_err(|e| crate::db::session::map_lost(s.as_ref(), e))?;
    Ok(QueryOutcome::Done { output })
}

#[tauri::command]
pub async fn db_cancel(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    let (s, _) = crate::db::session::get(&state, &session_id).await?;
    Ok(s.cancel().await?)
}

#[tauri::command]
pub async fn db_tx_state(state: State<'_, AppState>, session_id: String) -> CmdResult<crate::db::TxState> {
    let (s, _) = crate::db::session::get(&state, &session_id).await?;
    Ok(s.tx_state().await?)
}

// ---------- backup ----------

/// Ask where to save, then write an encrypted backup there. Returns the path,
/// or None when the user cancelled.
#[tauri::command]
pub async fn backup_export(app: AppHandle, state: State<'_, AppState>, passphrase: String, include_secrets: bool) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let backup = crate::backup::build(&state, include_secrets)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Save Shellport backup")
        .set_file_name(crate::backup::default_name())
        .add_filter("Shellport backup", &[crate::backup::EXTENSION])
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(picked) = rx.await.ok().flatten() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| anyhow!("unsupported file location: {e}"))?;
    let p2 = path.clone();
    let pass = zeroize::Zeroizing::new(passphrase);
    tokio::task::spawn_blocking(move || crate::backup::write(&p2, &pass, &backup)).await??;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Pick a backup file to restore (remembered as granted, like other picks).
#[tauri::command]
pub async fn backup_pick(app: AppHandle) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Restore Shellport backup")
        .add_filter("Shellport backup", &[crate::backup::EXTENSION])
        .pick_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(picked) = rx.await.ok().flatten() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| anyhow!("unsupported file location: {e}"))?;
    app.state::<AppState>().granted_files.lock().insert(std::fs::canonicalize(&path)?);
    Ok(Some(path.to_string_lossy().into_owned()))
}

async fn read_backup(state: &AppState, path: &str, passphrase: String) -> anyhow::Result<crate::backup::Backup> {
    let p = granted_path(state, std::path::Path::new(path), false)?;
    let pass = zeroize::Zeroizing::new(passphrase);
    tokio::task::spawn_blocking(move || crate::backup::read(&p, &pass)).await?
}

/// Decrypt a picked backup and say what restoring it would add.
#[tauri::command]
pub async fn backup_preview(state: State<'_, AppState>, path: String, passphrase: String) -> CmdResult<crate::backup::Summary> {
    let b = read_backup(&state, &path, passphrase).await?;
    Ok(crate::backup::summarize(&state, &b))
}

#[tauri::command]
pub async fn backup_import(state: State<'_, AppState>, path: String, passphrase: String, replace_settings: bool) -> CmdResult<crate::backup::Restored> {
    let b = read_backup(&state, &path, passphrase).await?;
    let r = crate::backup::apply(&state, b, replace_settings, allowed_shell).map_err(crate::backup::friendly)?;
    granted_path(&state, std::path::Path::new(&path), true).ok();
    Ok(r)
}

// ---------- terminals ----------

#[tauri::command]
pub fn term_open_local(app: AppHandle, cols: u16, rows: u16, cwd: Option<String>, on_event: Channel<TermEvent>) -> CmdResult<String> {
    Ok(term::open_local(app, cols, rows, cwd, on_event)?)
}

#[tauri::command]
pub async fn term_open_ssh(app: AppHandle, host_id: String, cols: u16, rows: u16, session_key: Option<String>, on_event: Channel<TermEvent>) -> CmdResult<String> {
    Ok(term::open_ssh(app, host_id, cols, rows, None, session_key, on_event).await?)
}

/// Pause or resume reading a terminal's output (the UI is behind on drawing).
#[tauri::command]
pub fn term_flow(app: AppHandle, id: String, paused: bool) {
    if let Ok(t) = term::get(&app, &id) {
        let _ = t.paused.send(paused);
    }
}

/// End the tmux session behind a closed tab.
#[tauri::command]
pub async fn term_kill_tmux(app: AppHandle, host_id: String, session_key: String) -> CmdResult<()> {
    Ok(term::kill_tmux(app, host_id, session_key).await?)
}

/// Check a pooled SSH connection still answers (after sleep or a network
/// change). A dead one is dropped, so its terminals report `lost` at once.
#[tauri::command]
pub async fn ssh_probe(state: State<'_, AppState>, host_id: String) -> CmdResult<bool> {
    Ok(crate::ssh::probe(&state, &host_id).await)
}

#[tauri::command]
pub async fn term_write(app: AppHandle, id: String, data: String) -> CmdResult<()> {
    Ok(term::get(&app, &id)?.write(data.as_bytes()).await?)
}

#[tauri::command]
pub async fn term_write_b64(app: AppHandle, id: String, data: String) -> CmdResult<()> {
    let bytes = B64.decode(data)?;
    Ok(term::get(&app, &id)?.write(&bytes).await?)
}

#[tauri::command]
pub async fn term_resize(app: AppHandle, id: String, cols: u16, rows: u16) -> CmdResult<()> {
    Ok(term::get(&app, &id)?.resize(cols, rows).await?)
}

#[tauri::command]
pub async fn term_close(app: AppHandle, id: String) -> CmdResult<()> {
    if let Ok(t) = term::get(&app, &id) {
        t.close().await;
    }
    Ok(())
}

// ---------- sftp ----------

#[tauri::command]
pub async fn sftp_home(app: AppHandle, host_id: String) -> CmdResult<String> {
    Ok(sftp::home(&app, &host_id).await?)
}

#[tauri::command]
pub async fn sftp_list(app: AppHandle, host_id: String, path: String) -> CmdResult<Vec<FileEntry>> {
    Ok(sftp::list(&app, &host_id, &path).await?)
}

#[tauri::command]
pub fn local_list(path: String) -> CmdResult<Vec<FileEntry>> {
    Ok(sftp::list_local(&path)?)
}

/// Open a native file picker from the backend. The picked file is recorded so
/// `local_read_text` / the ssh_config commands may read it; nothing else can be
/// read by path from the webview.
#[tauri::command]
pub async fn local_pick_file(app: AppHandle, title: String, directory: Option<String>) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut d = app.dialog().file().set_title(title);
    if let Some(dir) = directory {
        d = d.set_directory(crate::paths::expand_tilde(&dir));
    }
    d.pick_file(move |p| {
        let _ = tx.send(p);
    });
    let Some(picked) = rx.await.ok().flatten() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| anyhow!("unsupported file location: {e}"))?;
    let canon = std::fs::canonicalize(&path)?;
    app.state::<AppState>().granted_files.lock().insert(canon);
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// The canonical path of `p` if the user picked it via `local_pick_file`.
fn granted_path(state: &AppState, p: &std::path::Path, consume: bool) -> anyhow::Result<std::path::PathBuf> {
    let canon = std::fs::canonicalize(p)?;
    let mut g = state.granted_files.lock();
    let ok = if consume { g.remove(&canon) } else { g.contains(&canon) };
    if !ok {
        return Err(anyhow!("{} must be chosen with the file picker", p.display()));
    }
    Ok(canon)
}

/// Read a small local text file the user just picked (private key import).
#[tauri::command]
pub fn local_read_text(state: State<'_, AppState>, path: String) -> CmdResult<zeroize::Zeroizing<String>> {
    let p = granted_path(&state, &crate::paths::expand_tilde(&path), true)?;
    let meta = std::fs::metadata(&p)?;
    if meta.len() > 256 * 1024 {
        return Err(anyhow!("file is too large to be a key").into());
    }
    Ok(zeroize::Zeroizing::new(std::fs::read_to_string(p)?))
}

#[tauri::command]
pub fn local_home() -> String {
    crate::paths::home_dir().to_string_lossy().into_owned()
}

#[tauri::command]
pub async fn sftp_mkdir(app: AppHandle, host_id: String, path: String) -> CmdResult<()> {
    Ok(sftp::mkdir(&app, &host_id, &path).await?)
}

#[tauri::command]
pub async fn sftp_rename(app: AppHandle, host_id: String, from: String, to: String) -> CmdResult<()> {
    Ok(sftp::rename(&app, &host_id, &from, &to).await?)
}

#[tauri::command]
pub async fn sftp_remove(app: AppHandle, host_id: String, path: String) -> CmdResult<()> {
    Ok(sftp::remove(&app, &host_id, &path).await?)
}

#[tauri::command]
pub async fn sftp_chmod(app: AppHandle, host_id: String, path: String, mode: u32) -> CmdResult<()> {
    Ok(sftp::chmod(&app, &host_id, &path, mode).await?)
}

/// Run a transfer as a cancellable task (`task_cancel(transfer_id)`).
async fn run_transfer<F>(app: &AppHandle, transfer_id: String, on_event: Channel<TransferEvent>, fut: F) -> CmdResult<()>
where
    F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let ev = on_event.clone();
    let h = tokio::spawn(fut);
    app.state::<AppState>().add_task(transfer_id.clone(), h.abort_handle());
    let res = h.await;
    app.state::<AppState>().tasks.lock().remove(&transfer_id);
    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            let _ = ev.send(TransferEvent::Error { message: format!("{e:#}") });
            Err(e.into())
        }
        Err(j) if j.is_cancelled() => {
            let _ = ev.send(TransferEvent::Error { message: "cancelled".into() });
            Ok(())
        }
        Err(j) => Err(anyhow!("transfer task failed: {j}").into()),
    }
}

#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    host_id: String,
    remote: String,
    local_dir: String,
    overwrite: Option<bool>,
    transfer_id: String,
    on_event: Channel<TransferEvent>,
) -> CmdResult<()> {
    let (a, ev) = (app.clone(), on_event.clone());
    let overwrite = overwrite.unwrap_or(false);
    run_transfer(&app, transfer_id, on_event, async move { sftp::download(&a, &host_id, &remote, &local_dir, overwrite, &ev).await }).await
}

#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    host_id: String,
    local: String,
    remote_dir: String,
    transfer_id: String,
    on_event: Channel<TransferEvent>,
) -> CmdResult<()> {
    let (a, ev) = (app.clone(), on_event.clone());
    run_transfer(&app, transfer_id, on_event, async move { sftp::upload(&a, &host_id, &local, &remote_dir, &ev).await }).await
}

// ---------- docker ----------

#[tauri::command]
pub async fn docker_info(app: AppHandle, endpoint: String) -> CmdResult<EndpointInfo> {
    docker::ensure_endpoint(&endpoint)?;
    Ok(docker::backend(&app, &endpoint).await?.info(&endpoint).await?)
}

#[tauri::command]
pub async fn docker_list(app: AppHandle, endpoint: String, all: bool) -> CmdResult<Vec<ContainerInfo>> {
    docker::ensure_endpoint(&endpoint)?;
    Ok(docker::backend(&app, &endpoint).await?.list(all).await?)
}

#[tauri::command]
pub async fn docker_images(app: AppHandle, endpoint: String) -> CmdResult<Vec<ImageInfo>> {
    Ok(docker::backend(&app, &endpoint).await?.images().await?)
}

#[tauri::command]
pub async fn docker_action(app: AppHandle, endpoint: String, id: String, action: Action) -> CmdResult<()> {
    Ok(docker::backend(&app, &endpoint).await?.action(&id, action).await?)
}

#[tauri::command]
pub async fn docker_inspect(app: AppHandle, endpoint: String, id: String) -> CmdResult<serde_json::Value> {
    Ok(docker::backend(&app, &endpoint).await?.inspect(&id).await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn docker_logs(
    app: AppHandle,
    endpoint: String,
    id: String,
    tail: u32,
    timestamps: bool,
    follow: bool,
    on_event: Channel<TermEvent>,
) -> CmdResult<String> {
    Ok(docker::stream_logs(app, endpoint, id, tail, timestamps, follow, on_event).await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn docker_exec(
    app: AppHandle,
    endpoint: String,
    id: String,
    shell: Option<String>,
    user: Option<String>,
    cols: u16,
    rows: u16,
    on_event: Channel<TermEvent>,
) -> CmdResult<String> {
    Ok(docker::exec_shell(app, endpoint, id, shell, user, cols, rows, on_event).await?)
}

#[tauri::command]
pub async fn docker_reset(state: State<'_, AppState>, endpoint: String) -> CmdResult<()> {
    state.dockers.lock().await.remove(&endpoint);
    Ok(())
}

// ---------- settings ----------

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
pub fn settings_save(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    if settings.local_shell != state.settings.lock().local_shell && !allowed_shell(&settings.local_shell) {
        return Err(anyhow!("{} is not a login shell listed in /etc/shells", settings.local_shell).into());
    }
    settings.save(&state.settings_path)?;
    *state.settings.lock() = settings.clone();
    Ok(settings)
}

// ---------- docker administration ----------

use crate::docker_admin::{self, NetworkInfo, PruneOptions, PruneResult, PruneTarget, UsageRow, VolumeInfo};

#[tauri::command]
pub async fn docker_disk_usage(app: AppHandle, endpoint: String) -> CmdResult<Vec<UsageRow>> {
    Ok(docker::backend(&app, &endpoint).await?.disk_usage().await?)
}

#[tauri::command]
pub async fn docker_prune(app: AppHandle, endpoint: String, target: PruneTarget, options: PruneOptions) -> CmdResult<PruneResult> {
    Ok(docker::backend(&app, &endpoint).await?.prune(target, options).await?)
}

#[tauri::command]
pub async fn docker_volumes(app: AppHandle, endpoint: String) -> CmdResult<Vec<VolumeInfo>> {
    Ok(docker::backend(&app, &endpoint).await?.volumes().await?)
}

#[tauri::command]
pub async fn docker_volume_create(app: AppHandle, endpoint: String, name: String) -> CmdResult<()> {
    Ok(docker::backend(&app, &endpoint).await?.volume_create(&name).await?)
}

#[tauri::command]
pub async fn docker_volume_remove(app: AppHandle, endpoint: String, name: String, force: bool) -> CmdResult<()> {
    Ok(docker::backend(&app, &endpoint).await?.volume_remove(&name, force).await?)
}

#[tauri::command]
pub async fn docker_volume_inspect(app: AppHandle, endpoint: String, name: String) -> CmdResult<serde_json::Value> {
    Ok(docker::backend(&app, &endpoint).await?.volume_inspect(&name).await?)
}

#[tauri::command]
pub async fn docker_networks(app: AppHandle, endpoint: String) -> CmdResult<Vec<NetworkInfo>> {
    Ok(docker::backend(&app, &endpoint).await?.networks().await?)
}

#[tauri::command]
pub async fn docker_network_create(app: AppHandle, endpoint: String, name: String, internal: bool) -> CmdResult<()> {
    Ok(docker::backend(&app, &endpoint).await?.network_create(&name, internal).await?)
}

#[tauri::command]
pub async fn docker_network_remove(app: AppHandle, endpoint: String, id: String) -> CmdResult<()> {
    Ok(docker::backend(&app, &endpoint).await?.network_remove(&id).await?)
}

#[tauri::command]
pub async fn docker_network_inspect(app: AppHandle, endpoint: String, id: String) -> CmdResult<serde_json::Value> {
    Ok(docker::backend(&app, &endpoint).await?.network_inspect(&id).await?)
}

#[tauri::command]
pub async fn docker_image_remove(app: AppHandle, endpoint: String, reference: String, force: bool) -> CmdResult<Vec<String>> {
    Ok(docker::backend(&app, &endpoint).await?.image_remove(&reference, force).await?)
}

#[tauri::command]
pub async fn docker_image_pull(
    app: AppHandle,
    endpoint: String,
    reference: String,
    secret_id: Option<String>,
    on_event: Channel<TermEvent>,
) -> CmdResult<String> {
    Ok(docker_admin::image_pull(app, endpoint, reference, secret_id, on_event).await?)
}

#[tauri::command]
pub async fn docker_project_action(app: AppHandle, endpoint: String, project: String, action: Action) -> CmdResult<usize> {
    let b = docker::backend(&app, &endpoint).await?;
    Ok(docker_admin::project_action(&b, &project, action).await?)
}

#[derive(serde::Deserialize)]
pub struct LogSource {
    id: String,
    name: String,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn docker_logs_multi(
    app: AppHandle,
    endpoint: String,
    containers: Vec<LogSource>,
    tail: u32,
    timestamps: bool,
    follow: bool,
    on_event: Channel<TermEvent>,
) -> CmdResult<String> {
    let list = containers.into_iter().map(|c| (c.id, c.name)).collect();
    Ok(docker_admin::stream_multi_logs(app, endpoint, list, tail, timestamps, follow, on_event).await?)
}

// ---------- session (open tabs) ----------

fn session_path() -> std::path::PathBuf {
    crate::paths::config_dir().join("session.json")
}

#[tauri::command]
pub fn session_load() -> serde_json::Value {
    std::fs::read(session_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn session_save(session: serde_json::Value) -> CmdResult<()> {
    crate::paths::write_private(&session_path(), &serde_json::to_vec(&session)?)?;
    Ok(())
}

/// Only programs listed in /etc/shells (or the user's $SHELL) may be the local shell.
fn allowed_shell(shell: &str) -> bool {
    if !std::path::Path::new(shell).is_absolute() {
        return false;
    }
    let listed = std::fs::read_to_string("/etc/shells").is_ok_and(|t| t.lines().map(str::trim).any(|l| l == shell));
    listed || std::env::var("SHELL").is_ok_and(|s| s == shell)
}

/// Open an http(s) URL in the user's browser. No shell is involved and other
/// schemes (file:, custom handlers) are refused.
#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) || url.chars().any(|c| c.is_control()) {
        return Err(anyhow!("only http(s) links can be opened").into());
    }
    std::process::Command::new("xdg-open").arg(&url).stdin(std::process::Stdio::null()).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn revealed_serializes_as_plain_strings() {
        let r = super::Revealed { value: zeroize::Zeroizing::new("v".into()), passphrase: Some(zeroize::Zeroizing::new("p".into())) };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"value":"v","passphrase":"p"}"#);
    }

    #[test]
    fn shell_allowlist() {
        assert!(super::allowed_shell("/bin/sh"));
        assert!(!super::allowed_shell("sh"));
        assert!(!super::allowed_shell("/usr/bin/python3"));
    }
}
