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

fn read_blocks(path: &Path, out: &mut Vec<Block>, depth: u8) {
    if depth > 8 {
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
                        read_blocks(&f, out, depth + 1);
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
    fn m(p: &[u8], h: &[u8]) -> bool {
        match (p.first(), h.first()) {
            (None, None) => true,
            (Some(b'*'), _) => m(&p[1..], h) || (!h.is_empty() && m(p, &h[1..])),
            (Some(b'?'), Some(_)) => m(&p[1..], &h[1..]),
            (Some(a), Some(b)) if a == b => m(&p[1..], &h[1..]),
            _ => false,
        }
    }
    m(pat.as_bytes(), host.as_bytes())
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
    read_blocks(path, &mut blocks, 0);
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
