//! Writing a host back into `~/.ssh/config` without disturbing the rest of
//! the file: comments, ordering, indentation, unknown settings and other
//! hosts stay exactly as they were. Only the keywords Shellport manages are
//! touched, and only inside that host's own `Host` block.

/// Values Shellport manages for one host. `None` leaves a keyword untouched;
/// `Some("")` removes its line.
#[derive(Debug, Default, Clone)]
pub struct Managed {
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<String>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
}

const KEYS: [&str; 5] = ["HostName", "User", "Port", "IdentityFile", "ProxyJump"];

impl Managed {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "HostName" => self.hostname.as_deref(),
            "User" => self.user.as_deref(),
            "Port" => self.port.as_deref(),
            "IdentityFile" => self.identity_file.as_deref(),
            "ProxyJump" => self.proxy_jump.as_deref(),
            _ => None,
        }
    }
}

/// The keyword of a config line (lowercased), if it has one.
fn keyword(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let end = t
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(t.len());
    Some(t[..end].to_ascii_lowercase())
}

fn host_patterns(line: &str) -> Vec<String> {
    let t = line.trim_start();
    let rest = t[4..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    rest.split_whitespace()
        .map(|p| p.trim_matches('"').to_string())
        .collect()
}

/// Rewrite `line`'s value, keeping its indentation, keyword spelling and separator.
fn replace_value(line: &str, value: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    let t = &line[indent_len..];
    let kw_end = t
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(t.len());
    let after = &t[kw_end..];
    let sep_len = after.len()
        - after
            .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
            .len();
    let sep = if sep_len == 0 { " " } else { &after[..sep_len] };
    format!(
        "{}{}{}{}",
        &line[..indent_len],
        &t[..kw_end],
        sep,
        quote(value)
    )
}

fn quote(v: &str) -> String {
    if v.contains(char::is_whitespace) {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

/// True when `alias` is valid as a `Host` pattern we can write.
pub fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && !alias
            .contains(|c: char| c.is_whitespace() || matches!(c, '*' | '?' | '!' | '"' | '#' | '='))
}

/// Whether `text` itself has a `Host <alias>` block (not via `Include`).
pub fn has_block(text: &str, alias: &str) -> bool {
    text.lines()
        .any(|l| keyword(l).as_deref() == Some("host") && host_patterns(l) == [alias.to_string()])
}

/// Return `text` with `alias`'s block updated (or appended if missing).
pub fn upsert_block(text: &str, alias: &str, m: &Managed) -> String {
    let lines: Vec<&str> = text.lines().collect();
    // Find the `Host <alias>` block (a block naming only this alias).
    let start = lines.iter().position(|l| {
        keyword(l).as_deref() == Some("host") && host_patterns(l) == [alias.to_string()]
    });
    let Some(start) = start else {
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("Host {alias}\n"));
        for k in KEYS {
            if let Some(v) = m.get(k).filter(|v| !v.is_empty()) {
                out.push_str(&format!("    {k} {}\n", quote(v)));
            }
        }
        return out;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| matches!(keyword(l).as_deref(), Some("host") | Some("match")))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    let mut block: Vec<String> = lines[start + 1..end]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let indent = block
        .iter()
        .find(|l| keyword(l).is_some())
        .map(|l| l[..l.len() - l.trim_start().len()].to_string())
        .unwrap_or_else(|| "    ".into());
    for k in KEYS {
        let Some(value) = m.get(k) else { continue };
        let lk = k.to_ascii_lowercase();
        match block
            .iter()
            .position(|l| keyword(l).as_deref() == Some(lk.as_str()))
        {
            Some(i) if value.is_empty() => {
                block.remove(i);
            }
            Some(i) => block[i] = replace_value(&block[i], value),
            None if value.is_empty() => {}
            None => {
                // After the block's last setting, before trailing blanks/comments.
                let at = block
                    .iter()
                    .rposition(|l| keyword(l).is_some())
                    .map(|i| i + 1)
                    .unwrap_or(0);
                block.insert(at, format!("{indent}{k} {}", quote(value)));
            }
        }
    }
    let mut out: Vec<String> = lines[..=start].iter().map(|s| s.to_string()).collect();
    out.extend(block);
    out.extend(lines[end..].iter().map(|s| s.to_string()));
    let mut s = out.join("\n");
    if text.ends_with('\n') || text.is_empty() {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = "# my servers\nHost *\n  ServerAliveInterval 30\n\nHost web\n  HostName 10.0.0.5   # old\n  User deploy\n  ForwardAgent yes\n\n# database box\nHost db db-alias\n  HostName 10.0.0.9\n";

    #[test]
    fn edits_in_place_and_keeps_everything_else() {
        let m = Managed {
            hostname: Some("10.0.0.6".into()),
            port: Some("2222".into()),
            user: Some("".into()),
            ..Default::default()
        };
        let out = upsert_block(CONF, "web", &m);
        assert!(out.contains("# my servers\nHost *\n  ServerAliveInterval 30\n"));
        assert!(
            out.contains("Host web\n  HostName 10.0.0.6\n  ForwardAgent yes\n  Port 2222\n"),
            "{out}"
        );
        assert!(!out.contains("User deploy"));
        assert!(out.contains("# database box\nHost db db-alias\n  HostName 10.0.0.9\n"));
    }

    #[test]
    fn appends_missing_hosts_and_skips_multi_alias_blocks() {
        // `db` only appears in a block with two names, so it gets its own block.
        let m = Managed {
            hostname: Some("10.0.0.10".into()),
            user: Some("pg user".into()),
            ..Default::default()
        };
        let out = upsert_block(CONF, "db", &m);
        assert!(out.starts_with(CONF));
        assert!(
            out.ends_with("\nHost db\n    HostName 10.0.0.10\n    User \"pg user\"\n"),
            "{out}"
        );
        assert_eq!(
            upsert_block(
                "",
                "x",
                &Managed {
                    hostname: Some("h".into()),
                    ..Default::default()
                }
            ),
            "Host x\n    HostName h\n"
        );
    }

    #[test]
    fn keeps_separators_and_case() {
        let out = upsert_block(
            "host web\n\thostname=old\n",
            "web",
            &Managed {
                hostname: Some("new".into()),
                ..Default::default()
            },
        );
        assert_eq!(out, "host web\n\thostname=new\n");
    }

    #[test]
    fn alias_validation() {
        assert!(valid_alias("web-1.prod"));
        assert!(!valid_alias("web *") && !valid_alias("") && !valid_alias("a#b"));
    }
}
