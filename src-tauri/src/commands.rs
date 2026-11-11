//! Thin #[tauri::command] wrappers.

use crate::docker::{self, Action, ContainerInfo, EndpointInfo, ImageInfo};
use crate::error::CmdResult;
use crate::sftp::{self, FileEntry};
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

/// Save text (e.g. a terminal's output) to a file the user picks.
#[tauri::command]
pub async fn text_save(app: AppHandle, title: String, file_name: String, text: String) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(file_name)
        .add_filter("Text", &["txt", "log"])
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(picked) = rx.await.ok().flatten() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| anyhow!("unsupported file location: {e}"))?;
    crate::paths::write_private(&path, text.as_bytes())?;
    Ok(Some(path.to_string_lossy().into_owned()))
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
    state.library.lock().host_deleted(&id)?;
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
    let (s, _, _) = crate::db::session::get(&state, &session_id).await?;
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
    let (s, safety, profile_id) = crate::db::session::get(&state, &session_id).await?;
    if safety == Safety::Confirm && !confirmed {
        let risks = crate::db::risk::classify(&sql);
        if !risks.is_empty() {
            return Ok(QueryOutcome::NeedsConfirmation { risks });
        }
    }
    let started = std::time::Instant::now();
    let res = s.query(&sql, row_limit.clamp(1, DB_MAX_ROWS)).await.map_err(|e| crate::db::session::map_lost(s.as_ref(), e));
    // Query history (M2.5): what ran, when, how long, how many rows.
    let name = state.dbs.lock().get(&profile_id).map(|d| d.name).unwrap_or_default();
    let mut entry = crate::activity::ActivityEntry::new(crate::activity::ActivityKind::Query, name, &sql, res.is_ok());
    entry.db_id = Some(profile_id);
    entry.duration_ms = Some(started.elapsed().as_millis() as u64);
    match &res {
        Ok(o) => entry.count = o.results.last().map(|r| if r.columns.is_empty() { r.rows_affected.unwrap_or(0) } else { r.rows.len() as u64 }),
        Err(e) => entry.message = Some(format!("{e:#}")),
    }
    state.activity.add(entry);
    Ok(QueryOutcome::Done { output: res? })
}

#[tauri::command]
pub async fn db_cancel(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    let (s, _, _) = crate::db::session::get(&state, &session_id).await?;
    Ok(s.cancel().await?)
}

#[tauri::command]
pub async fn db_tx_state(state: State<'_, AppState>, session_id: String) -> CmdResult<crate::db::TxState> {
    let (s, _, _) = crate::db::session::get(&state, &session_id).await?;
    Ok(s.tx_state().await?)
}

// ---------- backup ----------

/// Ask where to save, then write an encrypted backup there. Returns the path,
/// or None when the user cancelled.
#[tauri::command]
pub async fn backup_export(app: AppHandle, state: State<'_, AppState>, passphrase: String, include_secrets: bool, include_history: Option<bool>) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let backup = crate::backup::build(&state, include_secrets, include_history.unwrap_or(false))?;
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

// ---------- library, activity, runs ----------

#[tauri::command]
pub fn library_get(state: State<'_, AppState>) -> crate::store::library::Library {
    state.library.lock().get()
}

#[tauri::command]
pub fn command_save(state: State<'_, AppState>, command: crate::store::library::SavedCommand) -> CmdResult<crate::store::library::SavedCommand> {
    Ok(state.library.lock().save_command(command)?)
}

#[tauri::command]
pub fn command_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.library.lock().delete_command(&id)?)
}

#[tauri::command]
pub fn workspace_save(state: State<'_, AppState>, workspace: crate::store::library::Workspace) -> CmdResult<crate::store::library::Workspace> {
    Ok(state.library.lock().save_workspace(workspace)?)
}

#[tauri::command]
pub fn workspace_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.library.lock().delete_workspace(&id)?)
}

#[tauri::command]
pub fn activity_list(state: State<'_, AppState>, filter: crate::activity::ActivityFilter) -> Vec<crate::activity::ActivityEntry> {
    state.activity.list(&filter)
}

#[tauri::command]
pub fn activity_delete(state: State<'_, AppState>, id: String) {
    state.activity.delete(&id)
}

#[tauri::command]
pub fn activity_clear(state: State<'_, AppState>, host_id: Option<String>, db_id: Option<String>) {
    state.activity.clear(host_id.as_deref(), db_id.as_deref())
}

/// Run a command on several hosts; results stream back per host.
// Async so the run is spawned on the Tokio runtime (sync commands run on the main thread).
#[tauri::command]
pub async fn hosts_run(
    app: AppHandle,
    host_ids: Vec<String>,
    command: String,
    timeout_secs: u64,
    confirmed_production: bool,
    on_event: Channel<crate::runner::RunEvent>,
) -> CmdResult<String> {
    Ok(crate::runner::start(app, host_ids, command, timeout_secs, confirmed_production, on_event)?)
}

/// Where a host's terminal logs are written.
#[tauri::command]
pub fn host_logs_dir(state: State<'_, AppState>, host_id: String) -> CmdResult<String> {
    let host = state.hosts.lock().get(&host_id)?;
    Ok(crate::term::log_dir(&host).to_string_lossy().into_owned())
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
pub async fn sftp_list(app: AppHandle, host_id: String, path: String, sudo: Option<bool>) -> CmdResult<Vec<FileEntry>> {
    Ok(sftp::list(&app, &host_id, &path, sudo.unwrap_or(false)).await?)
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
pub async fn sftp_mkdir(app: AppHandle, host_id: String, path: String, sudo: Option<bool>) -> CmdResult<()> {
    Ok(sftp::mkdir(&app, &host_id, &path, sudo.unwrap_or(false)).await?)
}

#[tauri::command]
pub async fn sftp_rename(app: AppHandle, host_id: String, from: String, to: String, sudo: Option<bool>) -> CmdResult<()> {
    Ok(sftp::rename(&app, &host_id, &from, &to, sudo.unwrap_or(false)).await?)
}

#[tauri::command]
pub async fn sftp_remove(app: AppHandle, host_id: String, path: String, sudo: Option<bool>) -> CmdResult<()> {
    Ok(sftp::remove(&app, &host_id, &path, sudo.unwrap_or(false)).await?)
}

#[tauri::command]
pub async fn sftp_chmod(app: AppHandle, host_id: String, path: String, mode: u32, sudo: Option<bool>) -> CmdResult<()> {
    Ok(sftp::chmod(&app, &host_id, &path, mode, sudo.unwrap_or(false)).await?)
}

// Editing remote text files (M3.2).

#[tauri::command]
pub async fn sftp_read_text(app: AppHandle, host_id: String, path: String, sudo: Option<bool>) -> CmdResult<crate::remote_edit::TextFile> {
    let s = sftp::session_as(&app, &host_id, sudo.unwrap_or(false)).await?;
    Ok(crate::remote_edit::read_text(&s, &path).await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // a Tauri command's arguments are its IPC fields
pub async fn sftp_write_text(
    app: AppHandle,
    host_id: String,
    path: String,
    text: String,
    expected_size: Option<u64>,
    expected_mtime: Option<u64>,
    force: Option<bool>,
    sudo: Option<bool>,
) -> CmdResult<crate::remote_edit::Saved> {
    let s = sftp::session_as(&app, &host_id, sudo.unwrap_or(false)).await?;
    let expected = expected_size.map(|size| (size, expected_mtime));
    Ok(crate::remote_edit::write_text(&s, &path, &text, expected, force.unwrap_or(false)).await?)
}

// Working as root, and compress/extract on the server (M3.3).

/// Start (or restart) the root SFTP session for a host. Without a password it
/// only works where sudo needs none; the password is used once and dropped.
#[tauri::command]
pub async fn sftp_sudo_open(app: AppHandle, host_id: String, password: Option<String>) -> CmdResult<()> {
    let pw = password.map(zeroize::Zeroizing::new);
    Ok(crate::sudo::open_sftp(&app, &host_id, pw).await?)
}

#[tauri::command]
pub async fn sftp_sudo_close(app: AppHandle, host_id: String) {
    crate::sudo::close_sftp(&app, &host_id).await
}

/// Run a compress/extract command as a cancellable task; records it in Activity.
async fn run_file_action(app: &AppHandle, host_id: String, what: String, cmd: String, sudo: bool, password: Option<String>, task_id: String) -> CmdResult<String> {
    let started = std::time::Instant::now();
    let a = app.clone();
    let hid = host_id.clone();
    let h = tokio::spawn(async move {
        let conn = crate::ssh::get_conn(a, hid, 0).await?;
        crate::sudo::run(&conn, &cmd, sudo, password.map(zeroize::Zeroizing::new)).await
    });
    let state = app.state::<AppState>();
    state.add_task(task_id.clone(), h.abort_handle());
    let res = h.await;
    state.tasks.lock().remove(&task_id);
    let outcome: Result<String, String> = match res {
        Ok(Ok((Some(0), out))) => Ok(out),
        Ok(Ok((code, out))) => Err(format!("{}{}", out.trim(), if out.trim().is_empty() { format!("exited with {}", code.map_or("no status".into(), |c| c.to_string())) } else { String::new() })),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(j) if j.is_cancelled() => Err("cancelled".into()),
        Err(j) => Err(format!("task failed: {j}")),
    };
    // A sudo password prompt isn't an outcome worth recording.
    if !matches!(&outcome, Err(m) if m.starts_with(crate::sudo::NEED_PASSWORD)) {
        let name = state.hosts.lock().get(&host_id).map(|h| h.name).unwrap_or_default();
        let mut e = crate::activity::ActivityEntry::new(crate::activity::ActivityKind::FileAction, name, &format!("{what}{}", if sudo { " (as root)" } else { "" }), outcome.is_ok());
        e.host_id = Some(host_id);
        e.message = outcome.as_ref().err().cloned();
        e.duration_ms = Some(started.elapsed().as_millis() as u64);
        state.activity.add(e);
    }
    outcome.map_err(|m| anyhow!(m).into())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn remote_compress(
    app: AppHandle,
    host_id: String,
    dir: String,
    names: Vec<String>,
    archive: String,
    format: crate::remote_ops::ArchiveFormat,
    sudo: Option<bool>,
    password: Option<String>,
    task_id: String,
) -> CmdResult<String> {
    let cmd = crate::remote_ops::compress_command(&dir, &names, &archive, format)?;
    let what = format!("Compress {} → {}", names.join(", "), sftp::join(&dir, &archive));
    run_file_action(&app, host_id, what, cmd, sudo.unwrap_or(false), password, task_id).await
}

/// Extract an archive into a new folder named after it (asks first if it exists).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn remote_extract(
    app: AppHandle,
    host_id: String,
    archive: String,
    overwrite: Option<bool>,
    sudo: Option<bool>,
    password: Option<String>,
    task_id: String,
) -> CmdResult<String> {
    let (overwrite, sudo) = (overwrite.unwrap_or(false), sudo.unwrap_or(false));
    let dest = crate::remote_ops::extract_folder(&archive);
    if !overwrite {
        let s = sftp::session_as(&app, &host_id, sudo).await?;
        if s.try_exists(dest.as_str()).await.unwrap_or(false) {
            return Err(anyhow!("{}{dest}", crate::remote_ops::DEST_EXISTS).into());
        }
    }
    let cmd = crate::remote_ops::extract_command(&archive, &dest, overwrite)?;
    run_file_action(&app, host_id, format!("Extract {archive} → {dest}"), cmd, sudo, password, task_id).await
}

// Local files for in-terminal transfers (M3.1): only what the user picks.

#[tauri::command]
pub async fn local_fs_pick(app: AppHandle, folder: bool, multiple: bool, write: bool, title: String) -> CmdResult<Vec<String>> {
    Ok(crate::local_fs::pick(&app, folder, multiple, write, &title).await?)
}

#[tauri::command]
pub fn local_fs_stat(app: AppHandle, path: String) -> CmdResult<crate::local_fs::Entry> {
    Ok(crate::local_fs::stat(&app, &path)?)
}

#[tauri::command]
pub fn local_fs_list(app: AppHandle, path: String) -> CmdResult<Vec<crate::local_fs::Entry>> {
    Ok(crate::local_fs::list(&app, &path)?)
}

/// Raw bytes (an ArrayBuffer on the web side).
#[tauri::command]
pub fn local_fs_read(app: AppHandle, path: String, offset: u64, len: u32) -> CmdResult<tauri::ipc::Response> {
    Ok(tauri::ipc::Response::new(crate::local_fs::read(&app, &path, offset, len.min(16 * 1024 * 1024))?))
}

#[tauri::command]
pub fn local_fs_create(app: AppHandle, dir: String, name: String, folder: bool) -> CmdResult<String> {
    Ok(crate::local_fs::create(&app, &dir, &name, folder)?)
}

/// Append raw bytes (the request body) to `x-path` (URL-encoded header); no base64.
#[tauri::command]
pub fn local_fs_append_raw(app: AppHandle, request: tauri::ipc::Request<'_>) -> CmdResult<()> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err(anyhow!("expected raw bytes").into()) };
    let path = request.headers().get("x-path").and_then(|v| v.to_str().ok()).ok_or_else(|| anyhow!("missing path"))?;
    let path = percent_decode(path);
    Ok(crate::local_fs::append(&app, &path, bytes)?)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tauri::command]
pub fn local_fs_append(app: AppHandle, path: String, data: String) -> CmdResult<()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| anyhow!("bad data: {e}"))?;
    Ok(crate::local_fs::append(&app, &path, &bytes)?)
}

// Open in my editor (M3.2).

#[tauri::command]
pub async fn local_edit_open(app: AppHandle, host_id: String, path: String) -> CmdResult<crate::local_edit::LocalEditView> {
    Ok(crate::local_edit::open(&app, &host_id, &path).await?)
}

#[tauri::command]
pub fn local_edit_list(app: AppHandle) -> Vec<crate::local_edit::LocalEditView> {
    crate::local_edit::list(&app)
}

#[tauri::command]
pub async fn local_edit_push(app: AppHandle, id: String, force: Option<bool>) {
    crate::local_edit::push(&app, &id, force.unwrap_or(false)).await
}

#[tauri::command]
pub fn local_edit_stop(app: AppHandle, id: String) {
    crate::local_edit::stop(&app, &id)
}

// Transfer queue (M3.1): one queue for uploads, downloads and server-to-server copies.
// Async so the queue can spawn work on the Tokio runtime (sync commands run on the main thread).

#[tauri::command]
pub async fn transfer_add(app: AppHandle, source: crate::transfers::Endpoint, dest_dir: crate::transfers::Endpoint, overwrite: Option<bool>) -> String {
    crate::transfers::add(&app, source, dest_dir, overwrite.unwrap_or(false))
}

#[tauri::command]
pub fn transfer_list(state: State<'_, AppState>) -> Vec<crate::transfers::TransferView> {
    state.transfers.list()
}

#[tauri::command]
pub async fn transfer_pause(app: AppHandle, id: String) {
    crate::transfers::pause(&app, &id)
}

#[tauri::command]
pub async fn transfer_resume(app: AppHandle, id: String, overwrite: Option<bool>) {
    crate::transfers::resume(&app, &id, overwrite.unwrap_or(false))
}

#[tauri::command]
pub async fn transfer_cancel(app: AppHandle, id: String) {
    crate::transfers::cancel(&app, &id)
}

#[tauri::command]
pub async fn transfer_start_now(app: AppHandle, id: String) {
    crate::transfers::start_now(&app, &id)
}

#[tauri::command]
pub fn transfer_clear(app: AppHandle, ids: Option<Vec<String>>) {
    crate::transfers::clear(&app, ids)
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
    fn decodes_header_paths() {
        assert_eq!(super::percent_decode("%2Fhome%2Fu%2Fa%20b%C3%A9.txt"), "/home/u/a bé.txt");
        assert_eq!(super::percent_decode("50%"), "50%");
        assert_eq!(super::percent_decode("%zz"), "%zz");
    }

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
