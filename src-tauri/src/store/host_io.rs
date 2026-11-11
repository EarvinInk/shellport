//! Import and export of hosts as JSON or CSV, for moving between machines or
//! from other tools. Secrets are never exported: hosts that log in with a
//! vault password or key come out without the secret reference.

use super::hosts::{AuthMethod, Host};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
struct JsonFile {
    shellport_hosts: u32,
    hosts: Vec<Host>,
}

const CSV_HEADER: &str = "name,group,hostname,port,username,auth,key_file,jump_host,tags,color,production";

/// Hosts without references to this machine's vault.
fn portable(mut h: Host) -> Host {
    h.auth = match h.auth {
        AuthMethod::Password { .. } => AuthMethod::Password { secret_id: None },
        AuthMethod::VaultKey { .. } => AuthMethod::VaultKey { secret_id: None },
        AuthMethod::KeyFile { path, .. } => AuthMethod::KeyFile { path, passphrase_secret_id: None },
        AuthMethod::Agent => AuthMethod::Agent,
    };
    h
}

pub fn export_json(hosts: &[Host]) -> Result<String> {
    let f = JsonFile { shellport_hosts: 1, hosts: hosts.iter().cloned().map(portable).collect() };
    Ok(serde_json::to_string_pretty(&f)?)
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn export_csv(hosts: &[Host]) -> String {
    let names: HashMap<&str, &str> = hosts.iter().map(|h| (h.id.as_str(), h.name.as_str())).collect();
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for h in hosts {
        let (auth, key) = match &h.auth {
            AuthMethod::Agent => ("agent", ""),
            AuthMethod::KeyFile { path, .. } => ("key_file", path.as_str()),
            AuthMethod::Password { .. } => ("password", ""),
            AuthMethod::VaultKey { .. } => ("vault_key", ""),
        };
        let jump = h.jump_host_id.as_deref().and_then(|j| names.get(j).copied()).unwrap_or("");
        let row = [
            h.name.as_str(),
            &h.group,
            &h.hostname,
            &h.port.to_string(),
            &h.username,
            auth,
            key,
            jump,
            &h.tags.join(";"),
            h.color.as_deref().unwrap_or(""),
            if h.production { "yes" } else { "" },
        ];
        out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// Split CSV text into rows (quotes, doubled quotes, newlines in quotes).
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                }
                row.clear();
            }
            (c, _) => field.push(c),
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(row);
    }
    rows
}

fn blank_host(name: String) -> Host {
    Host {
        id: String::new(),
        name,
        group: String::new(),
        hostname: String::new(),
        port: 22,
        username: String::new(),
        auth: AuthMethod::Agent,
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
        ssh_config_alias: None,
        log_terminal: false,
        favorite_paths: Vec::new(),
        zmodem: false,
    }
}

/// Parsed hosts plus, for each, the *name* of its jump host (resolved later).
pub struct Imported {
    pub hosts: Vec<Host>,
    pub jumps: Vec<Option<String>>,
}

pub fn parse(text: &str) -> Result<Imported> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with('{') {
        let f: JsonFile = serde_json::from_str(t).map_err(|e| anyhow::anyhow!("not a Shellport hosts file: {e}"))?;
        let names: HashMap<String, String> = f.hosts.iter().map(|h| (h.id.clone(), h.name.clone())).collect();
        let jumps = f.hosts.iter().map(|h| h.jump_host_id.as_ref().and_then(|j| names.get(j).cloned())).collect();
        let hosts = f.hosts.into_iter().map(|h| Host { jump_host_id: None, ..portable(h) }).collect();
        return Ok(Imported { hosts, jumps });
    }
    let mut rows = csv_rows(t).into_iter();
    let Some(header) = rows.next() else { bail!("the file is empty") };
    let col: HashMap<String, usize> = header.iter().enumerate().map(|(i, h)| (h.trim().to_lowercase(), i)).collect();
    let Some(&hcol) = ["hostname", "host", "address", "ip"].iter().find_map(|k| col.get(*k)) else { bail!("CSV needs a \"hostname\" column (columns: {CSV_HEADER})") };
    // Accept common alternative column names from other tools.
    let alias = |k: &str| -> &'static [&'static str] {
        match k {
            "username" => &["username", "user", "login"],
            "key_file" => &["key_file", "identityfile", "identity_file", "key"],
            "group" => &["group", "folder"],
            "name" => &["name", "label", "alias"],
            "jump_host" => &["jump_host", "proxyjump", "jump"],
            "port" => &["port"],
            "auth" => &["auth"],
            "tags" => &["tags"],
            "color" => &["color"],
            "production" => &["production"],
            _ => &[],
        }
    };
    let get = |r: &Vec<String>, k: &str| {
        alias(k).iter().find_map(|a| col.get(*a)).and_then(|&i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default()
    };
    let mut out = Imported { hosts: vec![], jumps: vec![] };
    for (n, r) in rows.enumerate() {
        let hostname = r.get(hcol).map(|s| s.trim().to_string()).unwrap_or_default();
        if hostname.is_empty() {
            continue;
        }
        let name = Some(get(&r, "name")).filter(|s| !s.is_empty()).unwrap_or_else(|| hostname.clone());
        let mut h = blank_host(name);
        h.hostname = hostname;
        h.group = get(&r, "group");
        h.username = get(&r, "username");
        if let Some(p) = Some(get(&r, "port")).filter(|p| !p.is_empty()) {
            h.port = p.parse().map_err(|_| anyhow::anyhow!("row {}: \"{p}\" is not a port", n + 2))?;
        }
        let key = get(&r, "key_file");
        h.auth = match get(&r, "auth").as_str() {
            "password" => AuthMethod::Password { secret_id: None },
            "vault_key" => AuthMethod::VaultKey { secret_id: None },
            "key_file" | "" if !key.is_empty() => AuthMethod::KeyFile { path: key, passphrase_secret_id: None },
            _ => AuthMethod::Agent,
        };
        h.tags = get(&r, "tags").split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        h.color = Some(get(&r, "color")).filter(|c| c.starts_with('#'));
        h.production = matches!(get(&r, "production").to_lowercase().as_str(), "yes" | "true" | "1");
        out.jumps.push(Some(get(&r, "jump_host")).filter(|j| !j.is_empty()));
        out.hosts.push(h);
    }
    if out.hosts.is_empty() {
        bail!("no hosts found in the file");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: &str, name: &str) -> Host {
        let mut h = blank_host(name.into());
        h.id = id.into();
        h.hostname = format!("{name}.example.com");
        h
    }

    #[test]
    fn csv_round_trip_without_secrets() {
        let mut a = host("1", "bastion");
        a.auth = AuthMethod::Password { secret_id: Some("secret".into()) };
        let mut b = host("2", "web, \"primary\"");
        b.jump_host_id = Some("1".into());
        b.port = 2222;
        b.production = true;
        b.tags = vec!["eu".into(), "web".into()];
        let csv = export_csv(&[a, b]);
        assert!(!csv.contains("secret"));
        let imp = parse(&csv).unwrap();
        assert_eq!(imp.hosts.len(), 2);
        let b2 = &imp.hosts[1];
        assert_eq!((b2.name.as_str(), b2.port, b2.production), ("web, \"primary\"", 2222, true));
        assert_eq!(b2.tags, vec!["eu", "web"]);
        assert_eq!(imp.jumps[1].as_deref(), Some("bastion"));
        assert_eq!(imp.hosts[0].auth, AuthMethod::Password { secret_id: None });
    }

    #[test]
    fn json_round_trip_drops_secret_ids() {
        let mut a = host("1", "a");
        a.auth = AuthMethod::VaultKey { secret_id: Some("k".into()) };
        let json = export_json(&[a]).unwrap();
        assert!(!json.contains("\"k\""));
        assert_eq!(parse(&json).unwrap().hosts[0].auth, AuthMethod::VaultKey { secret_id: None });
    }

    #[test]
    fn minimal_csv_and_errors() {
        let imp = parse("Host,User\r\n10.0.0.1,root\r\n\r\n").unwrap();
        assert_eq!((imp.hosts[0].hostname.as_str(), imp.hosts[0].username.as_str(), imp.hosts[0].name.as_str()), ("10.0.0.1", "root", "10.0.0.1"));
        assert!(parse("name,user\nx,y\n").is_err());
        assert!(parse("hostname,port\nh,abc\n").is_err());
    }
}
