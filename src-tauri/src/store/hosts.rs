use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Password stored in the vault.
    Password {
        #[serde(rename = "secretId")]
        secret_id: Option<String>,
    },
    /// Private key stored in the vault (secret of kind private_key).
    VaultKey {
        #[serde(rename = "secretId")]
        secret_id: Option<String>,
    },
    /// Private key file on disk, with optional passphrase from the vault.
    KeyFile {
        path: String,
        #[serde(rename = "passphraseSecretId")]
        passphrase_secret_id: Option<String>,
    },
    /// ssh-agent via $SSH_AUTH_SOCK.
    Agent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForward {
    pub id: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    #[serde(default)]
    pub auto_start: bool,
}

/// What a terminal does when its connection drops unexpectedly.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OnDisconnect {
    #[default]
    Reconnect,
    Ask,
    Close,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    pub hostname: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// Connect through another saved host (ProxyJump).
    #[serde(default)]
    pub jump_host_id: Option<String>,
    /// Remote Docker socket path (defaults to /var/run/docker.sock).
    #[serde(default)]
    pub docker_socket: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub forwards: Vec<PortForward>,
    #[serde(default)]
    pub keepalive_secs: Option<u64>,
    /// Command to run after login (e.g. `cd /srv && exec bash`).
    #[serde(default)]
    pub startup_command: Option<String>,
    #[serde(default)]
    pub on_disconnect: OnDisconnect,
    /// Run each terminal inside a tmux session that survives disconnects.
    #[serde(default)]
    pub tmux_session: bool,
}

fn default_port() -> u16 {
    22
}

#[derive(Default, Serialize, Deserialize)]
struct HostsFile {
    hosts: Vec<Host>,
}

pub struct HostStore {
    path: PathBuf,
    hosts: Vec<Host>,
}

impl HostStore {
    pub fn load(path: PathBuf) -> Result<Self> {
        let hosts = if path.exists() {
            let f: HostsFile = serde_json::from_slice(&std::fs::read(&path)?)?;
            f.hosts
        } else {
            Vec::new()
        };
        Ok(HostStore { path, hosts })
    }

    fn save(&self) -> Result<()> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let f = HostsFile { hosts: self.hosts.clone() };
        crate::paths::write_private(&self.path, &serde_json::to_vec_pretty(&f)?)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<Host> {
        let mut v = self.hosts.clone();
        v.sort_by(|a, b| (a.group.to_lowercase(), a.name.to_lowercase()).cmp(&(b.group.to_lowercase(), b.name.to_lowercase())));
        v
    }

    pub fn get(&self, id: &str) -> Result<Host> {
        match self.hosts.iter().find(|h| h.id == id) {
            Some(h) => Ok(h.clone()),
            None => bail!("host not found"),
        }
    }

    pub fn upsert(&mut self, mut host: Host) -> Result<Host> {
        if host.hostname.trim().is_empty() {
            bail!("hostname is required");
        }
        if host.name.trim().is_empty() {
            host.name = host.hostname.clone();
        }
        if host.id.is_empty() {
            host.id = uuid::Uuid::new_v4().to_string();
        }
        if host.jump_host_id.as_deref() == Some(host.id.as_str()) {
            bail!("a host cannot jump through itself");
        }
        for f in host.forwards.iter_mut() {
            if f.id.is_empty() {
                f.id = uuid::Uuid::new_v4().to_string();
            }
        }
        match self.hosts.iter_mut().find(|h| h.id == host.id) {
            Some(h) => *h = host.clone(),
            None => self.hosts.push(host.clone()),
        }
        self.save()?;
        Ok(host)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        self.hosts.retain(|h| h.id != id);
        for h in self.hosts.iter_mut() {
            if h.jump_host_id.as_deref() == Some(id) {
                h.jump_host_id = None;
            }
        }
        self.save()
    }

    pub fn has(&self, id: &str) -> bool {
        self.hosts.iter().any(|h| h.id == id)
    }

    /// Add hosts from a backup, keeping their ids; existing ids are skipped.
    pub fn merge(&mut self, hosts: Vec<Host>) -> Result<usize> {
        let mut n = 0;
        for h in hosts {
            if !self.has(&h.id) {
                self.hosts.push(h);
                n += 1;
            }
        }
        if n > 0 {
            self.save()?;
        }
        Ok(n)
    }

    /// Add many hosts at once, skipping ones whose name already exists.
    pub fn import(&mut self, hosts: Vec<Host>) -> Result<usize> {
        let mut n = 0;
        for mut h in hosts {
            if self.hosts.iter().any(|e| e.name == h.name) {
                continue;
            }
            h.id = uuid::Uuid::new_v4().to_string();
            self.hosts.push(h);
            n += 1;
        }
        self.save()?;
        Ok(n)
    }
}
