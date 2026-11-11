//! Local file access for in-terminal transfers (trzsz, ZModem; M3.1). The web
//! side may only touch files and folders the user picked in a native dialog
//! here, and what's inside picked folders; symlinks are never followed.

use crate::state::AppState;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// One picked root: a file or folder, readable, or writable too.
#[derive(Clone, Debug)]
pub struct Grant {
    root: PathBuf,
    write: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Is `canon` (canonical) inside a grant that allows this access?
fn allowed(grants: &[Grant], canon: &Path, write: bool) -> bool {
    grants.iter().any(|g| (!write || g.write) && canon.starts_with(&g.root))
}

fn check(app: &AppHandle, path: &str, write: bool) -> Result<PathBuf> {
    let p = Path::new(path);
    let meta = std::fs::symlink_metadata(p).with_context(|| format!("reading {path}"))?;
    if meta.file_type().is_symlink() {
        bail!("{path} is a symlink; Shellport doesn't follow those here");
    }
    let canon = std::fs::canonicalize(p)?;
    if !allowed(&app.state::<AppState>().local_grants.lock(), &canon, write) {
        bail!("{path} wasn't chosen in a file dialog");
    }
    Ok(canon)
}

/// Show a native picker; picked paths become readable (and writable for `write`).
pub async fn pick(app: &AppHandle, folder: bool, multiple: bool, write: bool, title: &str) -> Result<Vec<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let d = app.dialog().file().set_title(title);
    match (folder, multiple) {
        (true, _) => d.pick_folder(move |p| {
            let _ = tx.send(p.into_iter().collect::<Vec<_>>());
        }),
        (false, true) => d.pick_files(move |p| {
            let _ = tx.send(p.unwrap_or_default());
        }),
        (false, false) => d.pick_file(move |p| {
            let _ = tx.send(p.into_iter().collect());
        }),
    }
    let picked = rx.await.unwrap_or_default();
    let mut out = Vec::new();
    for fp in picked {
        let path = fp.into_path().map_err(|e| anyhow!("unsupported file location: {e}"))?;
        let canon = std::fs::canonicalize(&path)?;
        app.state::<AppState>().local_grants.lock().push(Grant { root: canon.clone(), write });
        out.push(canon.to_string_lossy().into_owned());
    }
    Ok(out)
}

fn entry(path: &Path, meta: &std::fs::Metadata) -> Entry {
    Entry {
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        is_dir: meta.is_dir(),
        size: if meta.is_dir() { 0 } else { meta.len() },
    }
}

pub fn stat(app: &AppHandle, path: &str) -> Result<Entry> {
    let p = check(app, path, false)?;
    Ok(entry(&p, &std::fs::metadata(&p)?))
}

/// A folder's files and subfolders (symlinks left out).
pub fn list(app: &AppHandle, path: &str) -> Result<Vec<Entry>> {
    let dir = check(app, path, false)?;
    let mut v = Vec::new();
    for e in std::fs::read_dir(&dir)? {
        let e = e?;
        let m = e.metadata()?;
        if m.file_type().is_symlink() {
            continue;
        }
        v.push(entry(&e.path(), &m));
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v)
}

pub fn read(app: &AppHandle, path: &str, offset: u64, len: u32) -> Result<Vec<u8>> {
    let p = check(app, path, false)?;
    let mut f = std::fs::File::open(&p)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::with_capacity(len as usize);
    f.take(len as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Create (or empty) `name` inside a writable picked folder; returns its path.
pub fn create(app: &AppHandle, dir: &str, name: &str, folder: bool) -> Result<String> {
    let d = check(app, dir, true)?;
    let name = crate::sftp::safe_component(name)?;
    let target = d.join(name);
    if let Ok(m) = std::fs::symlink_metadata(&target) {
        if m.file_type().is_symlink() {
            bail!("{} is a symlink; not writing through it", target.display());
        }
    }
    if folder {
        if !target.is_dir() {
            std::fs::create_dir(&target).with_context(|| format!("creating {}", target.display()))?;
        }
    } else {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.custom_flags(libc::O_NOFOLLOW);
        }
        o.open(&target).with_context(|| format!("creating {}", target.display()))?;
    }
    Ok(target.to_string_lossy().into_owned())
}

pub fn append(app: &AppHandle, path: &str, data: &[u8]) -> Result<()> {
    let p = check(app, path, true)?;
    let mut f = std::fs::OpenOptions::new().append(true).open(&p)?;
    f.write_all(data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_cover_only_their_tree() {
        let g = vec![Grant { root: "/home/u/Downloads".into(), write: true }, Grant { root: "/home/u/notes.txt".into(), write: false }];
        assert!(allowed(&g, Path::new("/home/u/Downloads/a/b.bin"), true));
        assert!(allowed(&g, Path::new("/home/u/notes.txt"), false));
        assert!(!allowed(&g, Path::new("/home/u/notes.txt"), true));
        assert!(!allowed(&g, Path::new("/home/u/Downloads2/x"), false));
        assert!(!allowed(&g, Path::new("/home/u/.ssh/id_ed25519"), false));
    }
}
