use crate::docker::DockerBackend;
use crate::ssh::SshConn;
use crate::db::session::OpenDb;
use crate::store::{databases::DbStore, hosts::HostStore, library::LibraryStore, settings::Settings};
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
    pub dbs: Mutex<DbStore>,
    /// Saved commands and workspaces.
    pub library: Mutex<LibraryStore>,
    /// Local history of queries, multi-host runs and transfers.
    pub activity: crate::activity::ActivityLog,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    /// Live SSH connections keyed by host id; shared by terminals, SFTP, Docker.
    pub conns: tokio::sync::Mutex<HashMap<String, Arc<SshConn>>>,
    pub sftp: tokio::sync::Mutex<HashMap<String, Arc<russh_sftp::client::SftpSession>>>,
    /// Remote files open in the user's own editor, by id.
    pub local_edits: Mutex<HashMap<String, crate::local_edit::LocalEdit>>,
    /// Root SFTP sessions (through sudo), keyed by host id.
    pub sudo_sftp: tokio::sync::Mutex<HashMap<String, Arc<russh_sftp::client::SftpSession>>>,
    /// Local files and folders picked for in-terminal transfers (trzsz, ZModem).
    pub local_grants: Mutex<Vec<crate::local_fs::Grant>>,
    /// The app-wide transfer queue.
    pub transfers: crate::transfers::Queue,
    pub dockers: tokio::sync::Mutex<HashMap<String, Arc<DockerBackend>>>,
    /// Open database sessions keyed by session id (one per DB tab).
    pub db_sessions: tokio::sync::Mutex<HashMap<String, OpenDb>>,
    /// Open terminal sessions (local, ssh, docker exec).
    pub terms: Mutex<HashMap<String, Arc<TermHandle>>>,
    /// Background tasks that can be cancelled (log streams, port forwards, transfers).
    pub tasks: Mutex<HashMap<String, AbortHandle>>,
    /// Host key confirmations waiting on the user.
    pub hostkey_pending: Arc<Mutex<HashMap<String, oneshot::Sender<crate::ssh::HostKeyDecision>>>>,
    /// Keyboard-interactive (2FA) prompts waiting on the user; None = cancelled.
    pub auth_pending: Mutex<HashMap<String, oneshot::Sender<Option<Vec<String>>>>>,
    /// Local files the user picked in a native file dialog opened by the
    /// backend; only these may be read by path from the webview.
    /// Per-host lock held while a connection is being established.
    pub connecting: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub granted_files: Mutex<std::collections::HashSet<PathBuf>>,
    /// Recent secret reveals, for rate limiting.
    pub reveals: Mutex<std::collections::VecDeque<std::time::Instant>>,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let dir = crate::paths::ensure_config_dir()?;
        let settings_path = dir.join("settings.json");
        Ok(AppState {
            vault: Mutex::new(Vault::new(dir.join("vault.bin"))),
            hosts: Mutex::new(HostStore::load(dir.join("hosts.json"))?),
            dbs: Mutex::new(DbStore::load(dir.join("databases.json"))?),
            library: Mutex::new(LibraryStore::load(dir.join("library.json"))?),
            activity: crate::activity::ActivityLog::load(dir.join("activity.json")),
            settings: Mutex::new(Settings::load(&settings_path)),
            settings_path,
            conns: Default::default(),
            sftp: Default::default(),
            transfers: Default::default(),
            local_grants: Default::default(),
            sudo_sftp: Default::default(),
            local_edits: Default::default(),
            dockers: Default::default(),
            db_sessions: Default::default(),
            terms: Default::default(),
            tasks: Default::default(),
            hostkey_pending: Default::default(),
            auth_pending: Default::default(),
            connecting: Default::default(),
            granted_files: Default::default(),
            reveals: Default::default(),
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
