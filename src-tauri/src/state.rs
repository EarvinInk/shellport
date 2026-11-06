use crate::docker::DockerBackend;
use crate::ssh::SshConn;
use crate::store::{hosts::HostStore, settings::Settings};
use crate::term::TermHandle;
use crate::vault::Vault;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

pub struct AppState {
    pub vault: Mutex<Vault>,
    pub hosts: Mutex<HostStore>,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    /// Live SSH connections keyed by host id; shared by terminals, SFTP, Docker.
    pub conns: tokio::sync::Mutex<HashMap<String, Arc<SshConn>>>,
    pub sftp: tokio::sync::Mutex<HashMap<String, Arc<russh_sftp::client::SftpSession>>>,
    pub dockers: tokio::sync::Mutex<HashMap<String, Arc<DockerBackend>>>,
    /// Open terminal sessions (local, ssh, docker exec).
    pub terms: Mutex<HashMap<String, Arc<TermHandle>>>,
    /// Background tasks that can be cancelled (log streams, port forwards, transfers).
    pub tasks: Mutex<HashMap<String, AbortHandle>>,
    /// Host key confirmations waiting on the user.
    pub hostkey_pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let dir = crate::paths::ensure_config_dir()?;
        let settings_path = dir.join("settings.json");
        Ok(AppState {
            vault: Mutex::new(Vault::new(dir.join("vault.bin"))),
            hosts: Mutex::new(HostStore::load(dir.join("hosts.json"))?),
            settings: Mutex::new(Settings::load(&settings_path)),
            settings_path,
            conns: Default::default(),
            sftp: Default::default(),
            dockers: Default::default(),
            terms: Default::default(),
            tasks: Default::default(),
            hostkey_pending: Default::default(),
        })
    }

    pub fn add_task(&self, id: String, h: AbortHandle) {
        if let Some(old) = self.tasks.lock().insert(id, h) {
            old.abort();
        }
    }

    pub fn cancel_task(&self, id: &str) -> bool {
        match self.tasks.lock().remove(id) {
            Some(h) => {
                h.abort();
                true
            }
            None => false,
        }
    }
}
