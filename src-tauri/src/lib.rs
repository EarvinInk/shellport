mod commands;
mod db;
mod docker;
mod docker_admin;
mod error;
mod glob;
mod known_hosts;
mod paths;
mod sftp;
mod ssh;
mod state;
mod store;
mod term;
mod vault;

use state::AppState;
use std::time::Duration;
use tauri::{Emitter, Manager};

/// Lock the vault after the configured idle period and tell the UI.
fn spawn_auto_lock(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let state = app.state::<AppState>();
            let minutes = state.settings.lock().auto_lock_minutes;
            if minutes == 0 {
                continue;
            }
            let locked = {
                let mut v = state.vault.lock();
                if v.is_unlocked() && v.last_activity.elapsed() > Duration::from_secs(minutes as u64 * 60) {
                    v.lock();
                    true
                } else {
                    false
                }
            };
            if locked {
                let _ = app.emit("vault-locked", ());
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new().expect("failed to initialise Shellport config directory");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            spawn_auto_lock(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_status,
            commands::vault_create,
            commands::vault_unlock,
            commands::vault_unlock_keyring,
            commands::vault_lock,
            commands::vault_forget_device,
            commands::vault_change_password,
            commands::secrets_list,
            commands::secret_save,
            commands::secret_delete,
            commands::secret_reveal,
            commands::secret_public_key,
            commands::secret_generate_key,
            commands::hosts_list,
            commands::host_save,
            commands::host_delete,
            commands::host_disconnect,
            commands::host_test,
            commands::hostkey_respond,
            commands::auth_prompt_respond,
            commands::forwards_active,
            commands::forward_stop,
            commands::ssh_config_preview,
            commands::ssh_config_import,
            commands::forward_start,
            commands::task_cancel,
            commands::tasks_active,
            commands::term_open_local,
            commands::term_open_ssh,
            commands::term_write,
            commands::term_write_b64,
            commands::term_resize,
            commands::term_close,
            commands::sftp_home,
            commands::sftp_list,
            commands::local_list,
            commands::local_home,
            commands::local_read_text,
            commands::local_pick_file,
            commands::open_url,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_chmod,
            commands::sftp_download,
            commands::sftp_upload,
            commands::docker_info,
            commands::docker_list,
            commands::docker_images,
            commands::docker_action,
            commands::docker_inspect,
            commands::docker_logs,
            commands::docker_exec,
            commands::docker_reset,
            commands::docker_disk_usage,
            commands::docker_prune,
            commands::docker_volumes,
            commands::docker_volume_create,
            commands::docker_volume_remove,
            commands::docker_volume_inspect,
            commands::docker_networks,
            commands::docker_network_create,
            commands::docker_network_remove,
            commands::docker_network_inspect,
            commands::docker_image_remove,
            commands::docker_image_pull,
            commands::docker_project_action,
            commands::docker_logs_multi,
            commands::settings_get,
            commands::settings_save,
            commands::session_load,
            commands::session_save,
            commands::db_engines,
            commands::dbs_list,
            commands::db_save,
            commands::db_delete,
            commands::db_test,
            commands::db_connect,
            commands::db_disconnect,
            commands::db_children,
            commands::db_query,
            commands::db_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Shellport");
}
