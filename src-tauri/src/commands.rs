//! Thin #[tauri::command] wrappers.

use crate::docker::{self, Action, ContainerInfo, EndpointInfo, ImageInfo};
use crate::error::CmdResult;
use crate::sftp::{self, FileEntry, TransferEvent};
use crate::state::AppState;
use crate::store::hosts::Host;
use crate::store::settings::Settings;
use crate::store::ssh_config::{self, SshConfigEntry};
use crate::term::{self, TermEvent};
use crate::vault::{self, SecretInput, SecretMeta, VaultStatus};
use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

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
    value: String,
    passphrase: Option<String>,
}

/// Explicit user action: show/copy a secret value.
#[tauri::command]
pub fn secret_reveal(state: State<'_, AppState>, id: String) -> CmdResult<Revealed> {
    let mut v = state.vault.lock();
    v.touch();
    let s = v.get(&id)?;
    Ok(Revealed { value: s.value.clone(), passphrase: s.passphrase.clone() })
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

#[tauri::command]
pub fn hosts_list(state: State<'_, AppState>) -> Vec<Host> {
    state.hosts.lock().list()
}

#[tauri::command]
pub fn host_save(state: State<'_, AppState>, host: Host) -> CmdResult<Host> {
    Ok(state.hosts.lock().upsert(host)?)
}

#[tauri::command]
pub async fn host_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    crate::ssh::disconnect(&state, &id).await;
    Ok(state.hosts.lock().delete(&id)?)
}

#[tauri::command]
pub fn ssh_config_preview(path: Option<String>) -> Vec<SshConfigEntry> {
    let p = path.map(|p| crate::paths::expand_tilde(&p)).unwrap_or_else(ssh_config::default_path);
    ssh_config::parse_file(&p)
}

#[tauri::command]
pub fn ssh_config_import(state: State<'_, AppState>, path: Option<String>, aliases: Vec<String>) -> CmdResult<usize> {
    let p = path.map(|p| crate::paths::expand_tilde(&p)).unwrap_or_else(ssh_config::default_path);
    let entries = ssh_config::parse_file(&p);
    let hosts: Vec<Host> = ssh_config::to_hosts(&entries).into_iter().filter(|h| aliases.contains(&h.name)).collect();
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
pub fn hostkey_respond(state: State<'_, AppState>, request_id: String, accept: bool) {
    if let Some(tx) = state.hostkey_pending.lock().remove(&request_id) {
        let _ = tx.send(accept);
    }
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

// ---------- terminals ----------

#[tauri::command]
pub fn term_open_local(app: AppHandle, cols: u16, rows: u16, cwd: Option<String>, on_event: Channel<TermEvent>) -> CmdResult<String> {
    Ok(term::open_local(app, cols, rows, cwd, on_event)?)
}

#[tauri::command]
pub async fn term_open_ssh(app: AppHandle, host_id: String, cols: u16, rows: u16, on_event: Channel<TermEvent>) -> CmdResult<String> {
    Ok(term::open_ssh(app, host_id, cols, rows, None, on_event).await?)
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

/// Read a small local text file (used to import private keys into the vault).
#[tauri::command]
pub fn local_read_text(path: String) -> CmdResult<String> {
    let p = crate::paths::expand_tilde(&path);
    let meta = std::fs::metadata(&p)?;
    if meta.len() > 256 * 1024 {
        return Err(anyhow!("file is too large to be a key").into());
    }
    Ok(std::fs::read_to_string(p)?)
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

#[tauri::command]
pub async fn sftp_download(app: AppHandle, host_id: String, remote: String, local_dir: String, on_event: Channel<TransferEvent>) -> CmdResult<()> {
    if let Err(e) = sftp::download(&app, &host_id, &remote, &local_dir, &on_event).await {
        let _ = on_event.send(TransferEvent::Error { message: format!("{e:#}") });
        return Err(e.into());
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_upload(app: AppHandle, host_id: String, local: String, remote_dir: String, on_event: Channel<TransferEvent>) -> CmdResult<()> {
    if let Err(e) = sftp::upload(&app, &host_id, &local, &remote_dir, &on_event).await {
        let _ = on_event.send(TransferEvent::Error { message: format!("{e:#}") });
        return Err(e.into());
    }
    Ok(())
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
pub async fn docker_image_pull(app: AppHandle, endpoint: String, reference: String, on_event: Channel<TermEvent>) -> CmdResult<String> {
    Ok(docker_admin::image_pull(app, endpoint, reference, on_event).await?)
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
