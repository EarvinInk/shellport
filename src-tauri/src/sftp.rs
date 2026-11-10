//! SFTP browsing and file transfer (the SCP use-case) over pooled SSH connections.

use crate::state::AppState;
use anyhow::{bail, Context, Result};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TransferEvent {
    #[serde(rename_all = "camelCase")]
    Progress { file: String, bytes: u64, total: u64, files_done: u32 },
    #[serde(rename_all = "camelCase")]
    Done { files: u32, bytes: u64 },
    Error { message: String },
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
    let s = Arc::new(SftpSession::new(ch.into_stream()).await.context("starting SFTP subsystem")?);
    state.sftp.lock().await.insert(host_id.to_string(), s.clone());
    Ok(s)
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn sort(v: &mut [FileEntry]) {
    v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

pub async fn home(app: &AppHandle, host_id: &str) -> Result<String> {
    Ok(session(app, host_id).await?.canonicalize(".").await?)
}

pub async fn list(app: &AppHandle, host_id: &str, path: &str) -> Result<Vec<FileEntry>> {
    let s = session(app, host_id).await?;
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
            mtime: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            permissions: perms,
        });
    }
    sort(&mut v);
    Ok(v)
}

pub async fn mkdir(app: &AppHandle, host_id: &str, path: &str) -> Result<()> {
    session(app, host_id).await?.create_dir(path).await?;
    Ok(())
}

pub async fn rename(app: &AppHandle, host_id: &str, from: &str, to: &str) -> Result<()> {
    session(app, host_id).await?.rename(from, to).await?;
    Ok(())
}

pub async fn chmod(app: &AppHandle, host_id: &str, path: &str, mode: u32) -> Result<()> {
    let s = session(app, host_id).await?;
    let mut attrs = russh_sftp::protocol::FileAttributes::empty();
    attrs.permissions = Some(mode & 0o7777);
    s.set_metadata(path, attrs).await?;
    Ok(())
}

pub async fn remove(app: &AppHandle, host_id: &str, path: &str) -> Result<()> {
    if path.trim().is_empty() || path == "/" {
        bail!("refusing to remove {path:?}");
    }
    let s = session(app, host_id).await?;
    remove_rec(&s, path.to_string()).await
}

fn remove_rec(s: &SftpSession, path: String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
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

struct Progress<'a> {
    out: &'a Channel<TransferEvent>,
    bytes: u64,
    total: u64,
    files: u32,
    last: Instant,
}

impl Progress<'_> {
    fn tick(&mut self, file: &str, force: bool) {
        if force || self.last.elapsed() > Duration::from_millis(150) {
            self.last = Instant::now();
            let _ = self.out.send(TransferEvent::Progress {
                file: file.to_string(),
                bytes: self.bytes,
                total: self.total,
                files_done: self.files,
            });
        }
    }
}

const CHUNK: usize = 256 * 1024;

/// Limits on what a single download may expand to (protects against a hostile
/// server returning an endless or looping tree).
const MAX_DEPTH: usize = 64;
const MAX_FILES: usize = 100_000;

/// Prefix of the error returned when a download would overwrite local files.
pub const EXISTS_PREFIX: &str = "local files exist:";

/// A server-supplied file name is only usable as a single local path component.
pub fn safe_component(name: &str) -> Result<&str> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        bail!("refusing unsafe file name from server: {name:?}");
    }
    Ok(name)
}

/// Collect (remote_path, local_path, size) for a download.
fn plan_download<'a>(
    s: &'a SftpSession,
    remote: String,
    local: PathBuf,
    depth: usize,
    out: &'a mut Vec<(String, PathBuf, u64)>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            bail!("folder is nested too deeply (over {MAX_DEPTH} levels)");
        }
        // lstat: a symlinked directory is not descended into (avoids loops);
        // a symlink to a file is downloaded as that file's contents.
        let lm = s.symlink_metadata(remote.as_str()).await?;
        let is_link = lm.file_type().is_symlink();
        let m = if is_link { s.metadata(remote.as_str()).await? } else { lm };
        if m.is_dir() {
            if is_link {
                return Ok(());
            }
            for e in s.read_dir(remote.as_str()).await? {
                let n = e.file_name();
                if n == "." || n == ".." {
                    continue;
                }
                let n = safe_component(&n)?.to_string();
                plan_download(s, join(&remote, &n), local.join(&n), depth + 1, out).await?;
            }
        } else {
            if out.len() >= MAX_FILES {
                bail!("too many files (over {MAX_FILES})");
            }
            out.push((remote, local, m.size.unwrap_or(0)));
        }
        Ok(())
    })
}

/// Refuse to write through a symlink anywhere between `base` and `path`.
fn check_no_symlinks(base: &Path, path: &Path) -> Result<()> {
    let rel = path.strip_prefix(base).context("download path escaped the target folder")?;
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

/// Create (or truncate) a local file without following a symlink at its path.
fn create_local(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    o.open(path)
}

pub async fn download(
    app: &AppHandle,
    host_id: &str,
    remote: &str,
    local_dir: &str,
    overwrite: bool,
    out: &Channel<TransferEvent>,
) -> Result<()> {
    let s = session(app, host_id).await?;
    let base = crate::paths::expand_tilde(local_dir);
    let name = remote.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("download");
    let dest = base.join(safe_component(name)?);
    let mut files = Vec::new();
    plan_download(&s, remote.to_string(), dest, 0, &mut files).await?;
    for (_, lpath, _) in &files {
        check_no_symlinks(&base, lpath)?;
    }
    if !overwrite {
        let existing = files.iter().filter(|f| std::fs::symlink_metadata(&f.1).is_ok()).count();
        if existing > 0 {
            bail!("{EXISTS_PREFIX}{existing}");
        }
    }
    let mut p = Progress { out, bytes: 0, total: files.iter().map(|f| f.2).sum(), files: 0, last: Instant::now() };
    let mut buf = vec![0u8; CHUNK];
    for (rpath, lpath, _) in &files {
        if let Some(d) = lpath.parent() {
            tokio::fs::create_dir_all(d).await?;
            check_no_symlinks(&base, lpath)?;
        }
        let mut rf = s.open(rpath.as_str()).await?;
        let mut lf = tokio::fs::File::from_std(create_local(lpath).with_context(|| format!("writing {}", lpath.display()))?);
        loop {
            let n = rf.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            lf.write_all(&buf[..n]).await?;
            p.bytes += n as u64;
            p.tick(rpath, false);
        }
        lf.flush().await?;
        p.files += 1;
        p.tick(rpath, true);
    }
    let _ = out.send(TransferEvent::Done { files: p.files, bytes: p.bytes });
    Ok(())
}

fn plan_upload(local: &Path, remote: String, dirs: &mut Vec<String>, files: &mut Vec<(PathBuf, String, u64)>) -> Result<()> {
    let m = std::fs::metadata(local)?;
    if m.is_dir() {
        dirs.push(remote.clone());
        for e in std::fs::read_dir(local)? {
            let e = e?;
            let n = e.file_name().to_string_lossy().into_owned();
            plan_upload(&e.path(), join(&remote, &n), dirs, files)?;
        }
    } else {
        files.push((local.to_path_buf(), remote, m.len()));
    }
    Ok(())
}

pub async fn upload(app: &AppHandle, host_id: &str, local: &str, remote_dir: &str, out: &Channel<TransferEvent>) -> Result<()> {
    let s = session(app, host_id).await?;
    let local = crate::paths::expand_tilde(local);
    let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "upload".into());
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    plan_upload(&local, join(remote_dir, &name), &mut dirs, &mut files)?;
    for d in &dirs {
        if !s.try_exists(d.as_str()).await.unwrap_or(false) {
            s.create_dir(d.as_str()).await?;
        }
    }
    let mut p = Progress { out, bytes: 0, total: files.iter().map(|f| f.2).sum(), files: 0, last: Instant::now() };
    let mut buf = vec![0u8; CHUNK];
    for (lpath, rpath, _) in &files {
        let mut lf = tokio::fs::File::open(lpath).await?;
        let mut rf = s
            .open_with_flags(rpath.as_str(), OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
            .await?;
        loop {
            let n = lf.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            rf.write_all(&buf[..n]).await?;
            p.bytes += n as u64;
            p.tick(rpath, false);
        }
        rf.shutdown().await?;
        // Preserve the executable bits.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = std::fs::metadata(lpath) {
                let mut attrs = russh_sftp::protocol::FileAttributes::empty();
                attrs.permissions = Some(m.permissions().mode() & 0o777);
                let _ = s.set_metadata(rpath.as_str(), attrs).await;
            }
        }
        p.files += 1;
        p.tick(rpath, true);
    }
    let _ = out.send(TransferEvent::Done { files: p.files, bytes: p.bytes });
    Ok(())
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
        assert!(create_local(&base.join("flink")).is_err(), "followed a file symlink");
        assert!(!outside.join("t").exists());
    }
}
