//! Editing remote text files (M3.2): read with size and binary checks, and save
//! back only if the file didn't change on the server meanwhile, without ever
//! leaving a half-written file behind.

use anyhow::{bail, Context, Result};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The editor opens files up to this size.
pub const MAX_EDIT: u64 = 5 * 1024 * 1024;
/// Prefix of the error returned when the file changed on the server since it was opened.
pub const CHANGED_PREFIX: &str = "changed on server:";

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    pub text: String,
    pub size: u64,
    pub mtime: Option<u64>,
    /// Not valid UTF-8: shown read-only so saving can't damage it.
    pub read_only: bool,
}

/// Size and modified time after a save (the new baseline for the next one).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub size: u64,
    pub mtime: Option<u64>,
}

/// Text from file bytes: refuses binary files, and marks non-UTF-8 text read-only.
fn decode(bytes: Vec<u8>) -> Result<(String, bool)> {
    if bytes[..bytes.len().min(8192)].contains(&0) {
        bail!("this looks like a binary file, so it can't be edited here; download it instead");
    }
    Ok(match String::from_utf8(bytes) {
        Ok(s) => (s, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    })
}

/// Did the file change since it was opened (or last saved)?
fn changed(expected: (u64, Option<u64>), now: (u64, Option<u64>)) -> bool {
    expected.0 != now.0 || (expected.1.is_some() && now.1.is_some() && expected.1 != now.1)
}

pub async fn read_text(s: &SftpSession, path: &str) -> Result<TextFile> {
    let m = s
        .metadata(path)
        .await
        .with_context(|| format!("reading {path}"))?;
    if m.is_dir() {
        bail!("{path} is a folder");
    }
    let size = m.size.unwrap_or(0);
    if size > MAX_EDIT {
        bail!("{path} is {:.1} MB; the editor opens files up to {} MB. Download it or use Open in my editor instead", size as f64 / 1e6, MAX_EDIT / 1_000_000);
    }
    let mut f = s
        .open(path)
        .await
        .with_context(|| format!("opening {path}"))?;
    let mut bytes = Vec::with_capacity(size as usize);
    (&mut f)
        .take(MAX_EDIT + 1)
        .read_to_end(&mut bytes)
        .await
        .with_context(|| format!("reading {path}"))?;
    if bytes.len() as u64 > MAX_EDIT {
        bail!("{path} grew past {} MB while reading", MAX_EDIT / 1_000_000);
    }
    let (text, read_only) = decode(bytes)?;
    Ok(TextFile {
        text,
        size,
        mtime: m.mtime.map(u64::from),
        read_only,
    })
}

fn dir_and_name(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(0) => ("/", &path[1..]),
        Some(i) => (&path[..i], &path[i + 1..]),
        None => (".", path),
    }
}

async fn stat(s: &SftpSession, path: &str) -> Option<FileAttributes> {
    s.metadata(path).await.ok()
}

async fn write_all(s: &SftpSession, path: &str, flags: OpenFlags, data: &[u8]) -> Result<()> {
    let mut f = s
        .open_with_flags(path, flags)
        .await
        .with_context(|| format!("writing {path}"))?;
    f.write_all(data)
        .await
        .with_context(|| format!("writing {path}"))?;
    f.shutdown()
        .await
        .with_context(|| format!("writing {path}"))?;
    Ok(())
}

/// Save `text` to `path` (see [`write_bytes`]).
pub async fn write_text(
    s: &SftpSession,
    path: &str,
    text: &str,
    expected: Option<(u64, Option<u64>)>,
    force: bool,
) -> Result<Saved> {
    write_bytes(s, path, text.as_bytes(), expected, force).await
}

/// Save `data` to `path`. Unless `force`, refuses when the file's size or
/// modified time differ from `expected` (someone else changed it).
pub async fn write_bytes(
    s: &SftpSession,
    path: &str,
    data: &[u8],
    expected: Option<(u64, Option<u64>)>,
    force: bool,
) -> Result<Saved> {
    let current = stat(s, path).await;
    if let (Some(exp), Some(cur), false) = (expected, &current, force) {
        let now = (cur.size.unwrap_or(0), cur.mtime.map(u64::from));
        if changed(exp, now) {
            bail!("{CHANGED_PREFIX} {path} was modified on the server after you opened it");
        }
    }
    let is_link = s
        .symlink_metadata(path)
        .await
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    let (dir, name) = dir_and_name(path);
    let tmp = format!("{}/.{name}.shellport-tmp", dir.trim_end_matches('/'));
    let old = format!("{}/.{name}.shellport-old", dir.trim_end_matches('/'));

    // Safe save: write a new file next to it, then swap it in. Only when that
    // keeps the file as it was: not through a symlink, and owned by us.
    let mut swapped = false;
    if let (Some(cur), false) = (&current, is_link) {
        if write_all(
            s,
            &tmp,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            data,
        )
        .await
        .is_ok()
        {
            let mine = stat(s, &tmp).await.and_then(|t| t.uid) == cur.uid;
            if mine {
                let mut attrs = FileAttributes::empty();
                attrs.permissions = cur.permissions.map(|p| p & 0o7777);
                let _ = s.set_metadata(tmp.as_str(), attrs).await;
                let _ = s.remove_file(old.as_str()).await;
                if s.rename(path, old.as_str()).await.is_ok() {
                    if s.rename(tmp.as_str(), path).await.is_ok() {
                        let _ = s.remove_file(old.as_str()).await;
                        swapped = true;
                    } else {
                        // Put the original back; the save then happens in place below.
                        s.rename(old.as_str(), path).await.with_context(|| {
                            format!("the original is safe at {old}, but couldn't be moved back")
                        })?;
                    }
                }
            }
            if !swapped {
                let _ = s.remove_file(tmp.as_str()).await;
            }
        }
    }
    if !swapped {
        // New files, symlinks, files owned by someone else, or a folder we
        // can't write to: write the file itself.
        write_all(
            s,
            path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            data,
        )
        .await?;
    }
    let after = stat(s, path)
        .await
        .context("saved, but couldn't read the file's new size")?;
    Ok(Saved {
        size: after.size.unwrap_or(0),
        mtime: after.mtime.map(u64::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_and_encoding() {
        assert_eq!(
            decode(b"hello\n".to_vec()).unwrap(),
            ("hello\n".into(), false)
        );
        assert!(decode(b"ELF\0\x01\x02".to_vec()).is_err());
        // Latin-1 text: opens, but read-only.
        let (t, ro) = decode(b"caf\xe9".to_vec()).unwrap();
        assert!(ro && t.starts_with("caf"));
        assert_eq!(decode(Vec::new()).unwrap(), (String::new(), false));
    }

    #[test]
    fn conflict_check() {
        assert!(!changed((10, Some(100)), (10, Some(100))));
        assert!(changed((10, Some(100)), (11, Some(100))));
        assert!(changed((10, Some(100)), (10, Some(101))));
        // A server that reports no modified time: compare sizes only.
        assert!(!changed((10, None), (10, Some(5))));
    }

    #[test]
    fn splits_paths() {
        assert_eq!(dir_and_name("/etc/hosts"), ("/etc", "hosts"));
        assert_eq!(dir_and_name("/hosts"), ("/", "hosts"));
        assert_eq!(dir_and_name("notes.txt"), (".", "notes.txt"));
    }
}
