//! Encrypted backup and restore of hosts, database profiles, settings and
//! (optionally) vault secrets, in one file the user keeps.
//!
//! The file uses the vault's format (Argon2id + XChaCha20-Poly1305, header
//! authenticated) with its own tag `SPB1` and a passphrase chosen at export,
//! independent of the master password.
//!
//! Restoring merges: items are matched by id, existing ones are kept as they
//! are, new ones are added with their ids, so hosts keep pointing at their
//! secrets and databases at their SSH hosts.

use crate::state::AppState;
use crate::store::{databases::DbProfile, hosts::Host, settings::Settings};
use crate::vault::{self, Secret};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroizing;

const MAGIC: &[u8; 4] = b"SPB1";
const VERSION: u32 = 1;
pub const EXTENSION: &str = "spbackup";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub version: u32,
    pub created_at: u64,
    pub app: String,
    pub hosts: Vec<Host>,
    pub databases: Vec<DbProfile>,
    pub settings: Settings,
    /// Present only when the user chose to include vault secrets.
    #[serde(default)]
    pub secrets: Option<Vec<Secret>>,
    /// Saved commands and workspaces.
    #[serde(default)]
    pub library: crate::store::library::Library,
    /// Activity & history, only when the user chose to include it.
    #[serde(default)]
    pub activity: Option<Vec<crate::activity::ActivityEntry>>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Collect everything to back up.
pub fn build(state: &AppState, include_secrets: bool, include_history: bool) -> Result<Backup> {
    let secrets = if include_secrets {
        let v = state.vault.lock();
        if !v.is_unlocked() {
            bail!("unlock the vault to include its secrets in the backup");
        }
        Some(v.export_secrets()?)
    } else {
        None
    };
    Ok(Backup {
        version: VERSION,
        created_at: now(),
        app: format!("shellport {}", env!("CARGO_PKG_VERSION")),
        hosts: state.hosts.lock().list(),
        databases: state.dbs.lock().list(),
        settings: state.settings.lock().clone(),
        secrets,
        library: state.library.lock().get(),
        activity: include_history.then(|| state.activity.all()),
    })
}

/// Encrypt and write. Slow on purpose (Argon2id): call off the async runtime.
pub fn write(path: &Path, passphrase: &str, backup: &Backup) -> Result<()> {
    if passphrase.chars().count() < 8 {
        bail!("use a passphrase of at least 8 characters");
    }
    let plain = Zeroizing::new(serde_json::to_vec(backup)?);
    let bytes = vault::seal(MAGIC, passphrase, &plain)?;
    crate::paths::write_private(path, &bytes)?;
    Ok(())
}

/// Read and decrypt. Slow on purpose (Argon2id): call off the async runtime.
pub fn read(path: &Path, passphrase: &str) -> Result<Backup> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let plain = vault::open_sealed(MAGIC, passphrase, &bytes)?;
    let b: Backup =
        serde_json::from_slice(&plain).context("the backup's contents could not be read")?;
    if b.version > VERSION {
        bail!(
            "this backup was made by a newer Shellport ({}); update Shellport to restore it",
            b.app
        );
    }
    Ok(b)
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub total: usize,
    /// Not present here yet (would be added).
    pub new: usize,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub created_at: u64,
    pub app: String,
    pub hosts: Counts,
    pub databases: Counts,
    pub commands: usize,
    pub workspaces: usize,
    /// History entries in the backup (0 when not included).
    pub history: usize,
    /// None when the backup has no secrets.
    pub secrets: Option<Counts>,
    pub vault_exists: bool,
    pub vault_unlocked: bool,
}

pub fn summarize(state: &AppState, b: &Backup) -> Summary {
    let hosts = state.hosts.lock();
    let dbs = state.dbs.lock();
    let v = state.vault.lock();
    Summary {
        created_at: b.created_at,
        app: b.app.clone(),
        hosts: Counts {
            total: b.hosts.len(),
            new: b.hosts.iter().filter(|h| !hosts.has(&h.id)).count(),
        },
        databases: Counts {
            total: b.databases.len(),
            new: b.databases.iter().filter(|d| !dbs.has(&d.id)).count(),
        },
        commands: b.library.commands.len(),
        workspaces: b.library.workspaces.len(),
        history: b.activity.as_ref().map_or(0, Vec::len),
        secrets: b.secrets.as_ref().map(|s| Counts {
            total: s.len(),
            // Unknown while locked: count them all as new.
            new: s.iter().filter(|x| !v.has_secret(&x.id)).count(),
        }),
        vault_exists: v.exists(),
        vault_unlocked: v.is_unlocked(),
    }
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub hosts: usize,
    pub databases: usize,
    pub secrets: usize,
    pub commands: usize,
    pub workspaces: usize,
    pub history: usize,
    pub settings: bool,
}

/// Merge a backup into this installation.
pub fn apply(
    state: &AppState,
    b: Backup,
    replace_settings: bool,
    allowed_shell: impl Fn(&str) -> bool,
) -> Result<Restored> {
    let mut r = Restored::default();
    if let Some(secrets) = b.secrets {
        let mut v = state.vault.lock();
        if !v.exists() {
            bail!("create a vault first (Vault tab), then restore, so the backup's secrets have somewhere to go");
        }
        if !v.is_unlocked() {
            bail!("unlock the vault to restore the backup's secrets");
        }
        r.secrets = v.import_secrets(secrets)?;
    }
    r.hosts = state.hosts.lock().merge(b.hosts)?;
    r.databases = state.dbs.lock().merge(b.databases)?;
    (r.commands, r.workspaces) = state.library.lock().merge(b.library)?;
    if let Some(a) = b.activity {
        r.history = state.activity.merge(a);
    }
    if replace_settings {
        let mut s = b.settings;
        // The shell setting must still be a login shell on this machine.
        let current = state.settings.lock().clone();
        if s.local_shell != current.local_shell && !allowed_shell(&s.local_shell) {
            s.local_shell = current.local_shell;
        }
        s.save(&state.settings_path)?;
        *state.settings.lock() = s;
        r.settings = true;
    }
    Ok(r)
}

/// Default file name for a new backup.
pub fn default_name() -> String {
    format!(
        "shellport-backup-{}.{EXTENSION}",
        chrono::Local::now().format("%Y-%m-%d")
    )
}

/// Errors to show as-is, without the anyhow chain noise.
pub fn friendly(e: anyhow::Error) -> anyhow::Error {
    anyhow!("{e:#}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(secrets: bool) -> Backup {
        Backup {
            version: VERSION,
            created_at: 1,
            app: "shellport test".into(),
            hosts: vec![],
            databases: vec![],
            settings: Settings::default(),
            secrets: secrets.then(Vec::new),
            library: Default::default(),
            activity: None,
        }
    }

    #[test]
    fn round_trip_and_wrong_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(format!("b.{EXTENSION}"));
        write(&p, "correct horse", &sample(true)).unwrap();
        let b = read(&p, "correct horse").unwrap();
        assert_eq!(b.app, "shellport test");
        assert!(b.secrets.is_some());
        let err = read(&p, "wrong passphrase").err().unwrap().to_string();
        assert!(err.contains("wrong passphrase"), "{err}");
    }

    #[test]
    fn tampering_and_wrong_file_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b");
        write(&p, "correct horse", &sample(false)).unwrap();
        let mut bytes = std::fs::read(&p).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&p, &bytes).unwrap();
        assert!(read(&p, "correct horse").is_err());
        // A vault file is not a backup.
        bytes[..4].copy_from_slice(b"SPV1");
        std::fs::write(&p, &bytes).unwrap();
        assert!(read(&p, "correct horse")
            .err()
            .unwrap()
            .to_string()
            .contains("not a Shellport backup"));
    }

    #[test]
    fn short_passphrase_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write(&dir.path().join("b"), "short", &sample(false)).is_err());
    }
}
