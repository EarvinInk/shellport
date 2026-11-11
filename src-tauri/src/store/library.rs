//! Saved commands and workspaces (`library.json`). Both are included in
//! backups; neither ever holds secrets.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a saved command is offered.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandScope {
    /// Empty = every host. Otherwise only these hosts…
    #[serde(default)]
    pub hosts: Vec<String>,
    /// …and/or hosts in these groups.
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedCommand {
    pub id: String,
    pub name: String,
    /// The command text; `{{name}}` or `{{name:default}}` are filled in when
    /// it is used. `{{host}}` and `{{user}}` come from the host.
    pub command: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub scope: CommandScope,
    /// Typed into new terminals on matching hosts after login.
    #[serde(default)]
    pub run_on_connect: bool,
}

/// A saved layout of terminal tabs and panes (opaque to the backend).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub layout: serde_json::Value,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    #[serde(default)]
    pub commands: Vec<SavedCommand>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
}

pub struct LibraryStore {
    path: PathBuf,
    lib: Library,
}

impl LibraryStore {
    pub fn load(path: PathBuf) -> Result<Self> {
        let lib = if path.exists() { serde_json::from_slice(&std::fs::read(&path)?)? } else { Library::default() };
        Ok(LibraryStore { path, lib })
    }

    fn save(&self) -> Result<()> {
        crate::paths::write_private(&self.path, &serde_json::to_vec_pretty(&self.lib)?)?;
        Ok(())
    }

    pub fn get(&self) -> Library {
        let mut l = self.lib.clone();
        l.commands.sort_by_key(|c| c.name.to_lowercase());
        l.workspaces.sort_by_key(|w| w.name.to_lowercase());
        l
    }

    pub fn save_command(&mut self, mut c: SavedCommand) -> Result<SavedCommand> {
        if c.command.trim().is_empty() {
            bail!("the command is empty");
        }
        if c.name.trim().is_empty() {
            c.name = c.command.lines().next().unwrap_or("").chars().take(40).collect();
        }
        if c.id.is_empty() {
            c.id = uuid::Uuid::new_v4().to_string();
        }
        match self.lib.commands.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c.clone(),
            None => self.lib.commands.push(c.clone()),
        }
        self.save()?;
        Ok(c)
    }

    pub fn delete_command(&mut self, id: &str) -> Result<()> {
        self.lib.commands.retain(|c| c.id != id);
        self.save()
    }

    pub fn save_workspace(&mut self, mut w: Workspace) -> Result<Workspace> {
        if w.name.trim().is_empty() {
            bail!("give the workspace a name");
        }
        if w.id.is_empty() {
            w.id = uuid::Uuid::new_v4().to_string();
        }
        match self.lib.workspaces.iter_mut().find(|x| x.id == w.id) {
            Some(x) => *x = w.clone(),
            None => self.lib.workspaces.push(w.clone()),
        }
        self.save()?;
        Ok(w)
    }

    pub fn delete_workspace(&mut self, id: &str) -> Result<()> {
        self.lib.workspaces.retain(|w| w.id != id);
        self.save()
    }

    /// A deleted host disappears from command scopes.
    pub fn host_deleted(&mut self, host_id: &str) -> Result<()> {
        let mut changed = false;
        for c in self.lib.commands.iter_mut() {
            let n = c.scope.hosts.len();
            c.scope.hosts.retain(|h| h != host_id);
            changed |= n != c.scope.hosts.len();
        }
        if changed {
            self.save()?;
        }
        Ok(())
    }

    /// Add items from a backup whose ids are new here; returns (commands, workspaces).
    pub fn merge(&mut self, other: Library) -> Result<(usize, usize)> {
        let mut n = (0, 0);
        for c in other.commands {
            if !self.lib.commands.iter().any(|x| x.id == c.id) {
                self.lib.commands.push(c);
                n.0 += 1;
            }
        }
        for w in other.workspaces {
            if !self.lib.workspaces.iter().any(|x| x.id == w.id) {
                self.lib.workspaces.push(w);
                n.1 += 1;
            }
        }
        if n != (0, 0) {
            self.save()?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_workspaces_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("library.json");
        let mut s = LibraryStore::load(p.clone()).unwrap();
        let c = s
            .save_command(SavedCommand {
                id: String::new(),
                name: String::new(),
                command: "systemctl status {{service:nginx}}".into(),
                description: String::new(),
                scope: CommandScope { hosts: vec!["h1".into()], groups: vec![] },
                run_on_connect: false,
            })
            .unwrap();
        assert_eq!(c.name, "systemctl status {{service:nginx}}");
        s.save_workspace(Workspace { id: String::new(), name: "Web".into(), layout: serde_json::json!({"tabs": []}) }).unwrap();
        s.host_deleted("h1").unwrap();
        let again = LibraryStore::load(p).unwrap().get();
        assert!(again.commands[0].scope.hosts.is_empty());
        assert_eq!(again.workspaces[0].name, "Web");
        assert!(LibraryStore::load(dir.path().join("x.json")).unwrap().save_workspace(Workspace { id: String::new(), name: " ".into(), layout: serde_json::Value::Null }).is_err());
    }
}
