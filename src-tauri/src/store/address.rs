//! Cleaning up what people paste into host fields, shared by SSH hosts and
//! database connections, plus parsing `postgres://` connection strings.

use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;

/// A host field after cleanup, with anything that was pulled out of it.
#[derive(Debug, Default, PartialEq)]
pub struct Address {
    pub host: String,
    /// `user@` found in front of the host.
    pub user: Option<String>,
    /// `:port` found after the host.
    pub port: Option<u16>,
}

/// Trim spaces, a leading `ssh://`-style scheme, a trailing `/` or path, and
/// split off `user@` and `:port`. Refuses values that can't be a host name.
pub fn clean_host(raw: &str) -> Result<Address> {
    let mut s = raw.trim();
    if let Some((_, rest)) = s.split_once("://") {
        s = rest;
    }
    // Anything after the first `/` (a path) is not part of the address.
    if let Some(i) = s.find('/') {
        s = &s[..i];
    }
    let mut out = Address::default();
    if let Some((u, h)) = s.rsplit_once('@') {
        if !u.is_empty() {
            out.user = Some(u.to_string());
        }
        s = h;
    }
    // [v6]:port, v4/name:port; a bare IPv6 address has several colons.
    if let Some(rest) = s.strip_prefix('[') {
        let Some((h, after)) = rest.split_once(']') else { bail!("unclosed [ in the host") };
        if let Some(p) = after.strip_prefix(':') {
            out.port = Some(p.parse().map_err(|_| anyhow::anyhow!("\"{p}\" is not a port number"))?);
        }
        s = h;
    } else if s.matches(':').count() == 1 {
        let (h, p) = s.split_once(':').unwrap();
        out.port = Some(p.parse().map_err(|_| anyhow::anyhow!("\"{p}\" is not a port number"))?);
        s = h;
    }
    if s.is_empty() {
        bail!("the host is empty");
    }
    if s.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | ',' | ';')) {
        bail!("\"{s}\" is not a valid host name or address");
    }
    out.host = s.to_string();
    Ok(out)
}

/// What a `postgres://` URL fills in on a database connection.
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbUrl {
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub username: String,
    /// Shown once so the UI can put it in the vault; never stored as is.
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    pub options: BTreeMap<String, String>,
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse `postgres[ql]://user:pass@host:port/db?sslmode=…`.
pub fn parse_db_url(raw: &str) -> Result<DbUrl> {
    let raw = raw.trim();
    let Some((scheme, rest)) = raw.split_once("://") else { bail!("expected a URL like postgres://user@host:5432/database") };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "postgres" | "postgresql") {
        bail!("only postgres:// URLs are supported for now");
    }
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let mut out = DbUrl { database: pct_decode(path), ..Default::default() };
    if let Some(u) = userinfo {
        let (user, pass) = u.split_once(':').map(|(a, b)| (a, Some(b))).unwrap_or((u, None));
        out.username = pct_decode(user);
        out.password = pass.map(pct_decode).filter(|p| !p.is_empty());
    }
    let addr = clean_host(&pct_decode(hostport))?;
    out.host = addr.host;
    out.port = addr.port;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = pct_decode(v);
        match k {
            "sslmode" => {
                out.ssl_mode = Some(
                    match v.as_str() {
                        "disable" => "disable",
                        "allow" | "prefer" => "prefer",
                        "require" | "verify-ca" => "require",
                        "verify-full" => "verify-full",
                        other => bail!("unknown sslmode \"{other}\""),
                    }
                    .to_string(),
                )
            }
            "application_name" => {
                out.options.insert("application_name".into(), v);
            }
            // Supabase/Prisma convention for transaction-mode poolers.
            "pgbouncer" if v == "true" => {
                out.options.insert("pooler".into(), "1".into());
            }
            "user" if out.username.is_empty() => out.username = v,
            "password" if out.password.is_none() => out.password = Some(v).filter(|p| !p.is_empty()),
            "dbname" if out.database.is_empty() => out.database = v,
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_pasted_hosts() {
        assert_eq!(clean_host(" db.internal/ ").unwrap().host, "db.internal");
        let a = clean_host("ssh://deploy@example.com:2222/home").unwrap();
        assert_eq!((a.host.as_str(), a.user.as_deref(), a.port), ("example.com", Some("deploy"), Some(2222)));
        assert_eq!(clean_host("[::1]:5432").unwrap(), Address { host: "::1".into(), user: None, port: Some(5432) });
        assert_eq!(clean_host("fe80::1").unwrap().host, "fe80::1");
        assert!(clean_host("my host").is_err());
        assert!(clean_host("host:abc").is_err());
        assert!(clean_host("  ").is_err());
    }

    #[test]
    fn parses_connection_strings() {
        let u = parse_db_url("postgresql://app%40corp:s%3Acret@db.example.com:6543/orders?sslmode=verify-full&pgbouncer=true&application_name=me").unwrap();
        assert_eq!(u.username, "app@corp");
        assert_eq!(u.password.as_deref(), Some("s:cret"));
        assert_eq!((u.host.as_str(), u.port, u.database.as_str()), ("db.example.com", Some(6543), "orders"));
        assert_eq!(u.ssl_mode.as_deref(), Some("verify-full"));
        assert_eq!(u.options.get("pooler").map(String::as_str), Some("1"));
        assert_eq!(u.options.get("application_name").map(String::as_str), Some("me"));
        let u = parse_db_url("postgres://localhost").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.username.as_str()), ("localhost", None, ""));
        assert!(parse_db_url("mysql://x").is_err());
        assert!(parse_db_url("postgres://h/db?sslmode=bogus").is_err());
    }
}
