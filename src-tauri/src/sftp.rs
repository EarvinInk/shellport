//! SFTP browsing and file transfer (the SCP use-case) over pooled SSH connections.

use crate::state::AppState;
use anyhow::{bail, Context, Result};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mtime: Option<u64>,
    pub permissions: Option<u32>,
}

pub async fn session(app: &AppHandle, host_id: &str) -> Result<Arc<SftpSession>> {
    let state = app.state::<AppState>();
    if let Some(s) = state.sftp.lock().await.get(host_id) {
        if s.canonicalize(".").await.is_ok() {
            return Ok(s.clone());
        }
    }
    let conn = crate::ssh::get_conn(app.clone(), host_id.to_string(), 0).await?;
    let ch = conn.handle.channel_open_session().await?;
    ch.request_subsystem(true, "sftp").await?;
    let s = Arc::new(
        SftpSession::new(ch.into_stream())
            .await
            .context("starting SFTP subsystem")?,
    );
    state
        .sftp
        .lock()
        .await
        .insert(host_id.to_string(), s.clone());
    Ok(s)
}

/// The normal SFTP session, or the root one (M3.3) when `sudo` is set.
pub async fn session_as(app: &AppHandle, host_id: &str, sudo: bool) -> Result<Arc<SftpSession>> {
    if sudo {
        crate::sudo::sftp(app, host_id).await
    } else {
        session(app, host_id).await
    }
}

pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn sort(v: &mut [FileEntry]) {
    v.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

pub async fn home(app: &AppHandle, host_id: &str) -> Result<String> {
    Ok(session(app, host_id).await?.canonicalize(".").await?)
}

pub async fn list(
    app: &AppHandle,
    host_id: &str,
    path: &str,
    sudo: bool,
) -> Result<Vec<FileEntry>> {
    let s = session_as(app, host_id, sudo).await?;
    let dir = s.canonicalize(path).await?;
    let mut v = Vec::new();
    for e in s.read_dir(&dir).await? {
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let m = e.metadata();
        let ft = e.file_type();
        let full = join(&dir, &name);
        let mut is_dir = ft.is_dir();
        if ft.is_symlink() {
            is_dir = s.metadata(&full).await.map(|t| t.is_dir()).unwrap_or(false);
        }
        v.push(FileEntry {
            name,
            path: full,
            is_dir,
            is_symlink: ft.is_symlink(),
            size: m.size.unwrap_or(0),
            mtime: m.mtime.map(u64::from),
            permissions: m.permissions,
        });
    }
    sort(&mut v);
    Ok(v)
}

pub fn list_local(path: &str) -> Result<Vec<FileEntry>> {
    let dir = crate::paths::expand_tilde(path);
    let mut v = Vec::new();
    for e in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let e = e?;
        let lmeta = e.path().symlink_metadata()?;
        let meta = std::fs::metadata(e.path()).unwrap_or_else(|_| lmeta.clone());
        #[cfg(unix)]
        let perms = {
            use std::os::unix::fs::PermissionsExt;
            Some(meta.permissions().mode())
        };
        #[cfg(not(unix))]
        let perms = None;
        v.push(FileEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            is_symlink: lmeta.file_type().is_symlink(),
            size: meta.len(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
            permissions: perms,
        });
    }
    sort(&mut v);
    Ok(v)
}

pub async fn mkdir(app: &AppHandle, host_id: &str, path: &str, sudo: bool) -> Result<()> {
    session_as(app, host_id, sudo)
        .await?
        .create_dir(path)
        .await?;
    Ok(())
}

pub async fn rename(
    app: &AppHandle,
    host_id: &str,
    from: &str,
    to: &str,
    sudo: bool,
) -> Result<()> {
    session_as(app, host_id, sudo)
        .await?
        .rename(from, to)
        .await?;
    Ok(())
}

pub async fn chmod(
    app: &AppHandle,
    host_id: &str,
    path: &str,
    mode: u32,
    sudo: bool,
) -> Result<()> {
    let s = session_as(app, host_id, sudo).await?;
    let mut attrs = russh_sftp::protocol::FileAttributes::empty();
    attrs.permissions = Some(mode & 0o7777);
    s.set_metadata(path, attrs).await?;
    Ok(())
}

pub async fn remove(app: &AppHandle, host_id: &str, path: &str, sudo: bool) -> Result<()> {
    if path.trim().is_empty() || path == "/" {
        bail!("refusing to remove {path:?}");
    }
    let s = session_as(app, host_id, sudo).await?;
    remove_rec(&s, path.to_string()).await
}

fn remove_rec(
    s: &SftpSession,
    path: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
    Box::pin(async move {
        let m = s.symlink_metadata(&path).await?;
        if m.is_dir() {
            for e in s.read_dir(&path).await? {
                let n = e.file_name();
                if n == "." || n == ".." {
                    continue;
                }
                remove_rec(s, join(&path, &n)).await?;
            }
            s.remove_dir(&path).await?;
        } else {
            s.remove_file(&path).await?;
        }
        Ok(())
    })
}

pub(crate) const CHUNK: usize = 256 * 1024;

/// Limits on what a single download may expand to (protects against a hostile
/// server returning an endless or looping tree).
pub(crate) const MAX_DEPTH: usize = 64;
pub(crate) const MAX_FILES: usize = 100_000;

/// A server-supplied file name is only usable as a single local path component.
pub fn safe_component(name: &str) -> Result<&str> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        bail!("refusing unsafe file name from server: {name:?}");
    }
    Ok(name)
}

/// Refuse to write through a symlink anywhere between `base` and `path`.
pub(crate) fn check_no_symlinks(base: &Path, path: &Path) -> Result<()> {
    let rel = path
        .strip_prefix(base)
        .context("download path escaped the target folder")?;
    let mut cur = base.to_path_buf();
    for c in rel.components() {
        match c {
            std::path::Component::Normal(_) => cur.push(c),
            _ => bail!("download path escaped the target folder"),
        }
        if let Ok(m) = std::fs::symlink_metadata(&cur) {
            if m.file_type().is_symlink() {
                bail!("refusing to write through symlink {}", cur.display());
            }
        }
    }
    Ok(())
}

/// Open a local destination file for writing without following a symlink at
/// its path; `truncate` empties it (a fresh copy), otherwise it is resumed.
pub(crate) fn open_local_dst(path: &Path, truncate: bool) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(truncate);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    o.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_names_rejected() {
        for bad in ["", ".", "..", "../x", "/etc/passwd", "a/b", "a\\b", "a\0b"] {
            assert!(safe_component(bad).is_err(), "{bad:?} accepted");
        }
        for ok in ["file.txt", "..hidden", "a b", "naïve.md"] {
            assert!(safe_component(ok).is_ok(), "{ok:?} rejected");
        }
    }

    #[test]
    fn no_writing_through_symlinks_or_outside() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("dl");
        std::fs::create_dir(&base).unwrap();
        assert!(check_no_symlinks(&base, &base.join("a/b.txt")).is_ok());
        assert!(check_no_symlinks(&base, &dir.path().join("elsewhere")).is_err());
        assert!(check_no_symlinks(&base, &base.join("../x")).is_err());
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("link")).unwrap();
        assert!(check_no_symlinks(&base, &base.join("link/f")).is_err());
        std::os::unix::fs::symlink(outside.join("t"), base.join("flink")).unwrap();
        assert!(
            open_local_dst(&base.join("flink"), true).is_err(),
            "followed a file symlink"
        );
        assert!(!outside.join("t").exists());
    }
}
