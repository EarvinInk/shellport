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

/// Collect (remote_path, local_path, size) for a download.
fn plan_download<'a>(
    s: &'a SftpSession,
    remote: String,
    local: PathBuf,
    out: &'a mut Vec<(String, PathBuf, u64)>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let m = s.metadata(&remote).await?;
        if m.is_dir() {
            std::fs::create_dir_all(&local)?;
            for e in s.read_dir(&remote).await? {
                let n = e.file_name();
                if n == "." || n == ".." {
                    continue;
                }
                plan_download(s, join(&remote, &n), local.join(&n), out).await?;
            }
        } else {
            out.push((remote, local, m.size.unwrap_or(0)));
        }
        Ok(())
    })
}

pub async fn download(app: &AppHandle, host_id: &str, remote: &str, local_dir: &str, out: &Channel<TransferEvent>) -> Result<()> {
    let s = session(app, host_id).await?;
    let name = remote.trim_end_matches('/').rsplit('/').next().unwrap_or("download");
    let dest = crate::paths::expand_tilde(local_dir).join(name);
    let mut files = Vec::new();
    plan_download(&s, remote.to_string(), dest, &mut files).await?;
    let mut p = Progress { out, bytes: 0, total: files.iter().map(|f| f.2).sum(), files: 0, last: Instant::now() };
    let mut buf = vec![0u8; CHUNK];
    for (rpath, lpath, _) in &files {
        if let Some(d) = lpath.parent() {
            tokio::fs::create_dir_all(d).await?;
        }
        let mut rf = s.open(rpath.as_str()).await?;
        let mut lf = tokio::fs::File::create(lpath).await?;
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
