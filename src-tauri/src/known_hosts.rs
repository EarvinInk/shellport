//! known_hosts lookup that fails safe.
//!
//! Unlike russh's helper this treats *any* saved key for the host (of any
//! algorithm) that differs from the presented one as a change, honours
//! `@revoked`, hashed names, wildcards and tab separators, and reports an
//! unreadable matching entry as a change rather than "unknown".

use crate::glob::glob_match;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::PublicKey;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The presented key is saved for this host.
    Known,
    /// Nothing is saved for this host.
    Unknown,
    /// Another key is saved for this host (or its entry can't be read).
    Changed { line: Option<usize> },
    /// The presented key is marked `@revoked`.
    Revoked { line: usize },
}

/// The name OpenSSH uses for host:port in known_hosts.
pub fn host_port(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

fn hashed_matches(entry: &str, name: &str) -> bool {
    // |1|base64(salt)|base64(HMAC-SHA1(salt, name))
    let mut parts = entry.split('|').skip(2);
    let (Some(salt), Some(hash)) = (parts.next(), parts.next()) else {
        return false;
    };
    let b64 = data_encoding::BASE64;
    let (Ok(salt), Ok(hash)) = (b64.decode(salt.as_bytes()), b64.decode(hash.as_bytes())) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.update(name.as_bytes());
    mac.verify_slice(&hash).is_ok()
}

/// Does a comma-separated known_hosts host field match `name`?
pub fn hosts_match(field: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let mut hit = false;
    for entry in field.split(',') {
        if entry.starts_with("|1|") {
            hit |= hashed_matches(entry, &name);
        } else if let Some(neg) = entry.strip_prefix('!') {
            if glob_match(&neg.to_ascii_lowercase(), &name) {
                return false;
            }
        } else if glob_match(&entry.to_ascii_lowercase(), &name) {
            hit = true;
        }
    }
    hit
}

pub fn check(host: &str, port: u16, key: &PublicKey, path: &Path) -> Verdict {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Verdict::Unknown,
        // Can't tell whether a different key is saved: assume it is.
        Err(_) => return Verdict::Changed { line: None },
    };
    check_text(&host_port(host, port), key, &text)
}

pub fn check_text(name: &str, key: &PublicKey, text: &str) -> Verdict {
    let mut other: Option<usize> = None;
    let mut known = false;
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tok = line.split_whitespace();
        let mut first = tok.next().unwrap_or_default();
        let marker = if first.starts_with('@') {
            let m = first;
            first = tok.next().unwrap_or_default();
            Some(m)
        } else {
            None
        };
        if marker == Some("@cert-authority") || !hosts_match(first, name) {
            continue;
        }
        let _algo = tok.next();
        let parsed = tok
            .next()
            .and_then(|b| russh::keys::parse_public_key_base64(b).ok());
        match (marker, parsed) {
            (Some("@revoked"), Some(k)) if k.key_data() == key.key_data() => {
                return Verdict::Revoked { line: line_no }
            }
            (Some(_), _) => {}
            (None, Some(k)) if k.key_data() == key.key_data() => known = true,
            (None, _) => {
                other.get_or_insert(line_no);
            }
        }
    }
    match (known, other) {
        (true, _) => Verdict::Known,
        (false, Some(l)) => Verdict::Changed { line: Some(l) },
        (false, None) => Verdict::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
    const ED2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const ECDSA: &str = "AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBDeR/8OZYyZcN7ZafsvO5NydsVKiNGgOhtoEUCe2uE6x8vJeQ7Yk9uwPEnPVBKfvDdA/5k5Op9mkiy2HelnAVOc=";

    fn k(b: &str) -> PublicKey {
        russh::keys::parse_public_key_base64(b).unwrap()
    }

    #[test]
    fn known_unknown_and_changed() {
        let kh = format!("example.com ssh-ed25519 {ED}\n");
        assert_eq!(check_text("example.com", &k(ED), &kh), Verdict::Known);
        assert_eq!(check_text("other.com", &k(ED), &kh), Verdict::Unknown);
        assert_eq!(
            check_text("example.com", &k(ED2), &kh),
            Verdict::Changed { line: Some(1) }
        );
    }

    #[test]
    fn different_algorithm_counts_as_changed() {
        let kh = format!("# c\nexample.com ssh-ed25519 {ED}\n");
        assert_eq!(
            check_text("example.com", &k(ECDSA), &kh),
            Verdict::Changed { line: Some(2) }
        );
    }

    #[test]
    fn tabs_ports_wildcards_and_negation() {
        let kh =
            format!("[10.0.0.5]:2222\tssh-ed25519\t{ED}\n*.corp,!bad.corp ssh-ed25519 {ED2}\n");
        assert_eq!(check_text("[10.0.0.5]:2222", &k(ED), &kh), Verdict::Known);
        assert_eq!(check_text("a.corp", &k(ED2), &kh), Verdict::Known);
        assert_eq!(check_text("bad.corp", &k(ED2), &kh), Verdict::Unknown);
        assert_eq!(check_text("10.0.0.5", &k(ED), &kh), Verdict::Unknown);
    }

    #[test]
    fn hashed_hostnames() {
        // `ssh-keygen -H` style entry for "example.com".
        use hmac::{Hmac, KeyInit, Mac};
        let salt = [7u8; 20];
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&salt).unwrap();
        mac.update(b"example.com");
        let b64 = data_encoding::BASE64;
        let entry = format!(
            "|1|{}|{}",
            b64.encode(&salt),
            b64.encode(&mac.finalize().into_bytes())
        );
        let kh = format!("{entry} ssh-ed25519 {ED}\n");
        assert_eq!(check_text("example.com", &k(ED), &kh), Verdict::Known);
        assert_eq!(check_text("example.org", &k(ED), &kh), Verdict::Unknown);
    }

    #[test]
    fn revoked_and_broken_entries() {
        let kh = format!("@revoked * ssh-ed25519 {ED}\nexample.com ssh-ed25519 {ED}\n");
        assert_eq!(
            check_text("example.com", &k(ED), &kh),
            Verdict::Revoked { line: 1 }
        );
        let broken = "example.com ssh-ed25519 notbase64!!\n";
        assert_eq!(
            check_text("example.com", &k(ED), broken),
            Verdict::Changed { line: Some(1) }
        );
        let ca = format!("@cert-authority * ssh-ed25519 {ED2}\n");
        assert_eq!(check_text("example.com", &k(ED), &ca), Verdict::Unknown);
    }
}
