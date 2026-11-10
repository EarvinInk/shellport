//! Saved database connection profiles (`databases.json`).

use crate::db::{DbEngine, SslMode};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    pub engine: DbEngine,
    /// Database server address, as seen from the SSH host when tunnelling.
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub username: String,
    /// Vault secret holding the password.
    #[serde(default)]
    pub password_secret_id: Option<String>,
    /// Reach the server through this saved SSH host.
    #[serde(default)]
    pub via_host_id: Option<String>,
    #[serde(default)]
    pub ssl_mode: SslMode,
    /// Engine-specific settings, so new engines need no schema change.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct DatabasesFile {
    databases: Vec<DbProfile>,
}

pub struct DbStore {
    path: PathBuf,
    dbs: Vec<DbProfile>,
}

impl DbStore {
    pub fn load(path: PathBuf) -> Result<Self> {
        let dbs = if path.exists() {
            let f: DatabasesFile = serde_json::from_slice(&std::fs::read(&path)?)?;
            f.databases
        } else {
            Vec::new()
        };
        Ok(DbStore { path, dbs })
    }

    fn save(&self) -> Result<()> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let f = DatabasesFile { databases: self.dbs.clone() };
        crate::paths::write_private(&self.path, &serde_json::to_vec_pretty(&f)?)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<DbProfile> {
        let mut v = self.dbs.clone();
        v.sort_by_key(|d| (d.group.to_lowercase(), d.name.to_lowercase()));
        v
    }

    pub fn get(&self, id: &str) -> Result<DbProfile> {
        match self.dbs.iter().find(|d| d.id == id) {
            Some(d) => Ok(d.clone()),
            None => bail!("database profile not found"),
        }
    }

    pub fn upsert(&mut self, mut db: DbProfile) -> Result<DbProfile> {
        db.host = db.host.trim().to_string();
        if db.host.is_empty() {
            bail!("host is required");
        }
        if db.port == 0 {
            bail!("port is required");
        }
        if db.name.trim().is_empty() {
            db.name = if db.database.is_empty() { db.host.clone() } else { format!("{}@{}", db.database, db.host) };
        }
        if db.id.is_empty() {
            db.id = uuid::Uuid::new_v4().to_string();
        }
        db.tags.retain(|t| !t.trim().is_empty());
        match self.dbs.iter_mut().find(|d| d.id == db.id) {
            Some(d) => *d = db.clone(),
            None => self.dbs.push(db.clone()),
        }
        self.save()?;
        Ok(db)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        self.dbs.retain(|d| d.id != id);
        self.save()
    }

    /// An SSH host was deleted: profiles that tunnelled through it now
    /// connect directly (the user sees this in the editor).
    pub fn host_deleted(&mut self, host_id: &str) -> Result<()> {
        let mut changed = false;
        for d in self.dbs.iter_mut() {
            if d.via_host_id.as_deref() == Some(host_id) {
                d.via_host_id = None;
                changed = true;
            }
        }
        if changed {
            self.save()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> DbProfile {
        DbProfile {
            id: String::new(),
            name: String::new(),
            group: String::new(),
            engine: DbEngine::Postgres,
            host: " db.internal ".into(),
            port: 5432,
            database: "app".into(),
            username: "app".into(),
            password_secret_id: None,
            via_host_id: Some("h1".into()),
            ssl_mode: SslMode::Prefer,
            options: BTreeMap::new(),
            color: None,
            tags: vec!["".into(), "prod".into()],
        }
    }

    #[test]
    fn round_trip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("databases.json");
        let mut s = DbStore::load(path.clone()).unwrap();
        let saved = s.upsert(profile()).unwrap();
        assert!(!saved.id.is_empty());
        assert_eq!(saved.host, "db.internal");
        assert_eq!(saved.name, "app@db.internal");
        assert_eq!(saved.tags, vec!["prod".to_string()]);

        let s2 = DbStore::load(path.clone()).unwrap();
        assert_eq!(s2.get(&saved.id).unwrap(), saved);

        // Older/minimal files still load.
        std::fs::write(&path, r#"{"databases":[{"id":"x","name":"n","engine":"postgres","host":"h","port":5432}]}"#).unwrap();
        let s3 = DbStore::load(path).unwrap();
        let d = s3.get("x").unwrap();
        assert_eq!(d.ssl_mode, SslMode::Prefer);
        assert!(d.via_host_id.is_none());
    }

    #[test]
    fn deleting_a_host_clears_tunnels() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DbStore::load(dir.path().join("databases.json")).unwrap();
        let saved = s.upsert(profile()).unwrap();
        s.host_deleted("h1").unwrap();
        assert!(s.get(&saved.id).unwrap().via_host_id.is_none());
    }

    #[test]
    fn host_is_required() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DbStore::load(dir.path().join("databases.json")).unwrap();
        let mut p = profile();
        p.host = "  ".into();
        assert!(s.upsert(p).is_err());
    }
}
