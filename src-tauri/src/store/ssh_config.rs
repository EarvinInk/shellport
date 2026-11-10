//! Minimal OpenSSH client config parser for importing hosts.
//! Supports Host, HostName, User, Port, IdentityFile, ProxyJump, Include and
//! `Host *` defaults. Wildcard host patterns are not imported as hosts.

use super::hosts::{AuthMethod, Host};
use crate::paths::{expand_tilde, home_dir};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SshConfigEntry {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
}

#[derive(Default)]
struct Block {
    patterns: Vec<String>,
    opts: HashMap<String, String>,
}

/// Caps on what one import may read, whatever the Include graph looks like.
const MAX_FILES: usize = 256;
const MAX_BLOCKS: usize = 10_000;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

fn read_blocks(path: &Path, out: &mut Vec<Block>, depth: u8, files: &mut usize) {
    if depth > 8 || *files >= MAX_FILES || out.len() >= MAX_BLOCKS {
        return;
    }
    *files += 1;
    if std::fs::metadata(path).map_or(true, |m| !m.is_file() || m.len() > MAX_FILE_BYTES) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let base = path.parent().map(Path::to_path_buf).unwrap_or_else(|| home_dir().join(".ssh"));
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, val) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim().trim_start_matches('=').trim().trim_matches('"').to_string()),
            None => continue,
        };
        match key.as_str() {
            _ if out.len() >= MAX_BLOCKS => return,
            "host" => out.push(Block {
                patterns: val.split_whitespace().map(String::from).collect(),
                opts: HashMap::new(),
            }),
            "match" => out.push(Block { patterns: vec![], opts: HashMap::new() }),
            "include" => {
                for pat in val.split_whitespace() {
                    let p = expand_tilde(pat);
                    let p = if p.is_absolute() { p } else { base.join(p) };
                    for f in glob_simple(&p) {
                        read_blocks(&f, out, depth + 1, files);
                    }
                }
            }
            _ => {
                if out.is_empty() {
                    out.push(Block { patterns: vec!["*".into()], opts: HashMap::new() });
                }
                // First value wins, like OpenSSH.
                out.last_mut().unwrap().opts.entry(key).or_insert(val);
            }
        }
    }
}

/// Supports a single `*` in the final path component (e.g. `config.d/*`).
fn glob_simple(p: &Path) -> Vec<PathBuf> {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.contains('*') {
        return vec![p.to_path_buf()];
    }
    let (pre, suf) = name.split_once('*').unwrap();
    let dir = p.parent().unwrap_or(Path::new("."));
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(pre) && n.ends_with(suf))
        })
        .collect();
    v.sort();
    v
}

fn pattern_matches(pat: &str, host: &str) -> bool {
    crate::glob::glob_match(pat, host)
}

fn block_matches(b: &Block, alias: &str) -> bool {
    let mut hit = false;
    for p in &b.patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if pattern_matches(neg, alias) {
                return false;
            }
        } else if pattern_matches(p, alias) {
            hit = true;
        }
    }
    hit
}

pub fn parse_file(path: &Path) -> Vec<SshConfigEntry> {
    let mut blocks = Vec::new();
    read_blocks(path, &mut blocks, 0, &mut 0);
    let mut aliases: Vec<String> = Vec::new();
    for b in &blocks {
        for p in &b.patterns {
            if !p.contains('*') && !p.contains('?') && !p.starts_with('!') && !aliases.contains(p) {
                aliases.push(p.clone());
            }
        }
    }
    aliases
        .into_iter()
        .map(|alias| {
            let mut opts: HashMap<String, String> = HashMap::new();
            for b in blocks.iter().filter(|b| block_matches(b, &alias)) {
                for (k, v) in &b.opts {
                    opts.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            SshConfigEntry {
                hostname: opts.get("hostname").map(|h| h.replace("%h", &alias)),
                user: opts.get("user").cloned(),
                port: opts.get("port").and_then(|p| p.parse().ok()),
                identity_file: opts.get("identityfile").cloned(),
                proxy_jump: opts.get("proxyjump").filter(|p| p.to_lowercase() != "none").cloned(),
                alias,
            }
        })
        .collect()
}

/// Hosts linked to `~/.ssh/config` take their address, user, port, key file
/// and jump host from the file (it is the source of truth). Returns the hosts
/// that changed. Password and vault-key logins are left alone.
pub fn refresh_linked(hosts: &[Host], entries: &[SshConfigEntry]) -> Vec<Host> {
    let mut changed = Vec::new();
    for h in hosts {
        let Some(alias) = h.ssh_config_alias.as_deref() else { continue };
        let Some(e) = entries.iter().find(|e| e.alias == alias) else { continue };
        let mut n = h.clone();
        n.hostname = e.hostname.clone().unwrap_or_else(|| alias.to_string());
        if let Some(u) = &e.user {
            n.username = u.clone();
        }
        n.port = e.port.unwrap_or(22);
        if matches!(n.auth, AuthMethod::KeyFile { .. } | AuthMethod::Agent) {
            let pass = match &n.auth {
                AuthMethod::KeyFile { passphrase_secret_id, .. } => passphrase_secret_id.clone(),
                _ => None,
            };
            n.auth = match &e.identity_file {
                Some(f) => AuthMethod::KeyFile { path: f.clone(), passphrase_secret_id: pass },
                None => AuthMethod::Agent,
            };
        }
        n.jump_host_id = match &e.proxy_jump {
            Some(pj) => {
                let first = pj.split(',').next().unwrap_or("");
                let name = first.rsplit_once('@').map(|(_, h)| h).unwrap_or(first);
                let name = name.split(':').next().unwrap_or(name);
                hosts.iter().find(|j| j.id != h.id && (j.ssh_config_alias.as_deref() == Some(name) || j.name == name)).map(|j| j.id.clone())
            }
            None => None,
        };
        if n.hostname != h.hostname || n.username != h.username || n.port != h.port || n.auth != h.auth || n.jump_host_id != h.jump_host_id {
            changed.push(n);
        }
    }
    changed
}

/// What Shellport writes back to `~/.ssh/config` for a linked host.
pub fn managed_for(host: &Host, jump_alias: Option<&str>) -> super::ssh_config_edit::Managed {
    use super::ssh_config_edit::Managed;
    Managed {
        hostname: Some(host.hostname.clone()),
        user: Some(host.username.clone()),
        port: Some(if host.port == 22 { String::new() } else { host.port.to_string() }),
        identity_file: match &host.auth {
            AuthMethod::KeyFile { path, .. } => Some(path.clone()),
            AuthMethod::Agent => Some(String::new()),
            // Vault keys and passwords live in Shellport only.
            _ => None,
        },
        proxy_jump: match (&host.jump_host_id, jump_alias) {
            (None, _) => Some(String::new()),
            (Some(_), Some(a)) => Some(a.to_string()),
            // The jump host isn't in the file: leave the line as it is.
            (Some(_), None) => None,
        },
    }
}

/// Write `host` back into the config file, keeping everything else intact.
/// A copy of the previous file is kept next to it.
pub fn write_linked(path: &Path, host: &Host, jump_alias: Option<&str>) -> anyhow::Result<()> {
    let alias = host.ssh_config_alias.as_deref().ok_or_else(|| anyhow::anyhow!("host is not linked to ~/.ssh/config"))?;
    let old = std::fs::read_to_string(path).unwrap_or_default();
    // Defined only in an included file: appending here would be shadowed by
    // the original, so don't pretend to have saved it.
    if !super::ssh_config_edit::has_block(&old, alias) && parse_file(path).iter().any(|e| e.alias == alias) {
        anyhow::bail!("\"{alias}\" is defined in a file included from {} (or shares a Host line with other names); edit it there, or turn off sync for this host", path.display());
    }
    let new = super::ssh_config_edit::upsert_block(&old, alias, &managed_for(host, jump_alias));
    if new == old {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if !old.is_empty() {
        crate::paths::write_private(&path.with_file_name("config.shellport-backup"), old.as_bytes())?;
    }
    crate::paths::write_private(path, new.as_bytes())?;
    Ok(())
}

pub fn default_path() -> PathBuf {
    home_dir().join(".ssh/config")
}

/// Convert parsed entries into Shellport hosts. ProxyJump references to other
/// imported aliases are resolved after ids are assigned by the caller.
pub fn to_hosts(entries: &[SshConfigEntry]) -> Vec<Host> {
    let default_user = std::env::var("USER").unwrap_or_else(|_| "root".into());
    let mut hosts: Vec<Host> = entries
        .iter()
        .map(|e| Host {
            id: uuid::Uuid::new_v4().to_string(),
            name: e.alias.clone(),
            group: "ssh config".into(),
            hostname: e.hostname.clone().unwrap_or_else(|| e.alias.clone()),
            port: e.port.unwrap_or(22),
            username: e.user.clone().unwrap_or_else(|| default_user.clone()),
            auth: match &e.identity_file {
                Some(f) => AuthMethod::KeyFile { path: f.clone(), passphrase_secret_id: None },
                None => AuthMethod::Agent,
            },
            jump_host_id: None,
            docker_socket: None,
            tags: vec![],
            color: None,
            forwards: vec![],
            keepalive_secs: None,
            startup_command: None,
            on_disconnect: Default::default(),
            tmux_session: false,
            production: false,
            notes: None,
            ssh_config_alias: Some(e.alias.clone()),
        })
        .collect();
    // Resolve ProxyJump (first hop only; `user@host:port` forms match by alias).
    let ids: HashMap<String, String> = hosts.iter().map(|h| (h.name.clone(), h.id.clone())).collect();
    for (h, e) in hosts.iter_mut().zip(entries) {
        if let Some(pj) = &e.proxy_jump {
            let first = pj.split(',').next().unwrap_or("");
            let name = first.rsplit('@').next().unwrap_or(first);
            let name = name.split(':').next().unwrap_or(name);
            h.jump_host_id = ids.get(name).cloned();
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_hosts_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, "# keep me\nHost bastion\n  HostName 1.2.3.4\n\nHost web\n  HostName 10.0.0.5\n  User deploy\n  ProxyJump bastion\n  ForwardAgent yes\n").unwrap();
        let mut hosts = to_hosts(&parse_file(&path));
        for (i, h) in hosts.iter_mut().enumerate() {
            h.id = format!("h{i}");
        }
        // Linked hosts take their values (and jump host) from the file.
        let changed = refresh_linked(&hosts, &parse_file(&path));
        let web = changed.iter().find(|h| h.name == "web").unwrap().clone();
        assert_eq!(web.jump_host_id.as_deref(), Some("h0"));
        // Edit in Shellport and write back: only the managed lines change.
        let mut web = web;
        web.hostname = "10.0.0.6".into();
        web.port = 2200;
        write_linked(&path, &web, Some("bastion")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# keep me\nHost bastion\n  HostName 1.2.3.4\n"), "{text}");
        assert!(text.contains("  HostName 10.0.0.6\n") && text.contains("  Port 2200\n") && text.contains("  ForwardAgent yes\n"), "{text}");
        assert!(dir.path().join("config.shellport-backup").exists());
        let again = parse_file(&path);
        let e = again.iter().find(|e| e.alias == "web").unwrap();
        assert_eq!((e.hostname.as_deref(), e.port), (Some("10.0.0.6"), Some(2200)));
        // A host that only exists in an included file is refused, not duplicated.
        std::fs::write(dir.path().join("extra"), "Host inc\n  HostName 9.9.9.9\n").unwrap();
        std::fs::write(&path, format!("Include {}\n", dir.path().join("extra").display())).unwrap();
        let mut inc = web.clone();
        inc.ssh_config_alias = Some("inc".into());
        assert!(write_linked(&path, &inc, None).unwrap_err().to_string().contains("included"));
    }

    #[test]
    fn include_bomb_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config");
        // Each file includes itself 8 times: unbounded this is 8^9 reads.
        let inc = format!("Include {0}\n", cfg.display()).repeat(8);
        std::fs::write(&cfg, format!("{inc}Host a\n  HostName 1.2.3.4\n")).unwrap();
        let t = std::time::Instant::now();
        let e = parse_file(&cfg);
        assert!(e.iter().any(|x| x.alias == "a"));
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn parses_hosts_defaults_and_include() {
        let dir = tempfile::tempdir().unwrap();
        let inc = dir.path().join("conf.d");
        std::fs::create_dir(&inc).unwrap();
        std::fs::write(inc.join("extra"), "Host extra\n  HostName 10.0.0.9\n").unwrap();
        let cfg = dir.path().join("config");
        std::fs::write(
            &cfg,
            format!(
                "Include {}/*\n\nHost bastion\n  HostName bastion.example.com\n  User ops\n\nHost web db\n  HostName %h.internal\n  Port 2222\n  ProxyJump bastion\n  IdentityFile ~/.ssh/id_ed25519\n\nHost *\n  User fallback\n  Port 22\n",
                inc.display()
            ),
        )
        .unwrap();
        let entries = parse_file(&cfg);
        let names: Vec<_> = entries.iter().map(|e| e.alias.as_str()).collect();
        assert_eq!(names, vec!["extra", "bastion", "web", "db"]);
        let web = entries.iter().find(|e| e.alias == "web").unwrap();
        assert_eq!(web.hostname.as_deref(), Some("web.internal"));
        assert_eq!(web.port, Some(2222));
        assert_eq!(web.user.as_deref(), Some("fallback"));
        let bastion = entries.iter().find(|e| e.alias == "bastion").unwrap();
        assert_eq!(bastion.user.as_deref(), Some("ops"));
        assert_eq!(bastion.port, Some(22));

        let hosts = to_hosts(&entries);
        let b = hosts.iter().find(|h| h.name == "bastion").unwrap();
        let w = hosts.iter().find(|h| h.name == "web").unwrap();
        assert_eq!(w.jump_host_id.as_deref(), Some(b.id.as_str()));
        assert!(matches!(w.auth, AuthMethod::KeyFile { .. }));
        assert!(matches!(b.auth, AuthMethod::Agent));
    }

    #[test]
    fn negation_and_wildcards() {
        assert!(pattern_matches("*.prod", "api.prod"));
        assert!(!pattern_matches("*.prod", "api.dev"));
        let b = Block { patterns: vec!["*".into(), "!secret".into()], opts: HashMap::new() };
        assert!(block_matches(&b, "web"));
        assert!(!block_matches(&b, "secret"));
    }
}
