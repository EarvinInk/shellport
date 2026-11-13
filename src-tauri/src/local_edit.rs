//! "Open in my editor" (M3.2): a remote file is copied to a private temp folder,
//! opened with the user's own editor, and uploaded each time it's saved there
//! (through the same "changed on the server?" check as the built-in editor).
//! Temp copies are removed when editing stops and when Shellport starts.

use crate::state::AppState;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncReadExt;

/// Event telling the UI what happened to a watched file.
pub const EVENT: &str = "local-edit";
/// Files bigger than this aren't opened this way.
const MAX_SIZE: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEditView {
    pub id: String,
    pub host_id: String,
    pub remote_path: String,
    /// "watching", "uploading", "uploaded", "conflict" or "error".
    pub state: String,
    pub message: Option<String>,
}

pub struct LocalEdit {
    view: LocalEditView,
    local: PathBuf,
    dir: PathBuf,
    /// Remote size and modified time we last saw (the conflict-check baseline).
    base: (u64, Option<u64>),
    task: Option<tokio::task::AbortHandle>,
}

fn tmp_root() -> PathBuf {
    crate::paths::config_dir().join("tmp")
}

/// Remove temp copies left by an earlier run.
pub fn clean_leftovers() {
    let _ = std::fs::remove_dir_all(tmp_root());
}

fn emit(app: &AppHandle, v: &LocalEditView) {
    let _ = app.emit(EVENT, v.clone());
}

fn update(app: &AppHandle, id: &str, f: impl FnOnce(&mut LocalEdit)) {
    let state = app.state::<AppState>();
    let v = {
        let mut m = state.local_edits.lock();
        let Some(e) = m.get_mut(id) else { return };
        f(e);
        e.view.clone()
    };
    emit(app, &v);
}

fn private_dir(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Copy the file down, open it in the user's editor, and start watching it.
pub async fn open(app: &AppHandle, host_id: &str, remote_path: &str) -> Result<LocalEditView> {
    let s = crate::sftp::session(app, host_id).await?;
    let m = s
        .metadata(remote_path)
        .await
        .with_context(|| format!("reading {remote_path}"))?;
    if m.is_dir() {
        bail!("{remote_path} is a folder");
    }
    let size = m.size.unwrap_or(0);
    if size > MAX_SIZE {
        bail!(
            "{remote_path} is too big to open this way (over {} MB); download it instead",
            MAX_SIZE / 1_000_000
        );
    }
    let name =
        crate::sftp::safe_component(remote_path.rsplit('/').next().unwrap_or("file"))?.to_string();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let root = tmp_root();
    private_dir(&root)?;
    let dir = root.join(&id);
    private_dir(&dir)?;
    let local = dir.join(&name);
    let mut bytes = Vec::with_capacity(size as usize);
    s.open(remote_path)
        .await?
        .read_to_end(&mut bytes)
        .await
        .with_context(|| format!("reading {remote_path}"))?;
    crate::paths::write_private(&local, &bytes)?;

    let view = LocalEditView {
        id: id.clone(),
        host_id: host_id.into(),
        remote_path: remote_path.into(),
        state: "watching".into(),
        message: None,
    };
    app.state::<AppState>().local_edits.lock().insert(
        id.clone(),
        LocalEdit {
            view: view.clone(),
            local: local.clone(),
            dir,
            base: (size, m.mtime.map(u64::from)),
            task: None,
        },
    );
    launch_editor(app, &local)?;
    let (a, id2) = (app.clone(), id.clone());
    let h = tokio::spawn(async move { watch(a, id2).await });
    if let Some(e) = app.state::<AppState>().local_edits.lock().get_mut(&id) {
        e.task = Some(h.abort_handle());
    }
    emit(app, &view);
    Ok(view)
}

/// Open the file with the editor command from Settings, or the desktop's default app.
fn launch_editor(app: &AppHandle, local: &std::path::Path) -> Result<()> {
    let cmd = app
        .state::<AppState>()
        .settings
        .lock()
        .external_editor
        .trim()
        .to_string();
    let mut c = if cmd.is_empty() {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(local);
        c
    } else {
        // The command runs in a shell (so "code --wait" works); the file's
        // path is passed as an argument, never pasted into the command text.
        let mut c = std::process::Command::new("sh");
        c.arg("-c")
            .arg(format!("{cmd} \"$1\""))
            .arg("shellport-editor")
            .arg(local);
        c
    };
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c.spawn().with_context(|| {
        if cmd.is_empty() {
            "couldn't run xdg-open".to_string()
        } else {
            format!("couldn't run the editor command \"{cmd}\"")
        }
    })?;
    Ok(())
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

/// Upload whenever the local copy is saved.
async fn watch(app: AppHandle, id: String) {
    let Some(local) = app
        .state::<AppState>()
        .local_edits
        .lock()
        .get(&id)
        .map(|e| e.local.clone())
    else {
        return;
    };
    let mut last = mtime(&local);
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = mtime(&local);
        if now.is_none() || now == last {
            continue;
        }
        // Let the editor finish writing.
        tokio::time::sleep(Duration::from_millis(300)).await;
        last = mtime(&local);
        push(&app, &id, false).await;
    }
}

/// Upload the local copy now (`force` skips the "changed on the server" check).
pub async fn push(app: &AppHandle, id: &str, force: bool) {
    let Some((host_id, remote, local, base)) =
        app.state::<AppState>().local_edits.lock().get(id).map(|e| {
            (
                e.view.host_id.clone(),
                e.view.remote_path.clone(),
                e.local.clone(),
                e.base,
            )
        })
    else {
        return;
    };
    update(app, id, |e| {
        e.view.state = "uploading".into();
        e.view.message = None;
    });
    let res = async {
        let data = tokio::fs::read(&local)
            .await
            .context("reading the local copy")?;
        let s = crate::sftp::session(app, &host_id).await?;
        crate::remote_edit::write_bytes(&s, &remote, &data, Some(base), force).await
    }
    .await;
    match res {
        Ok(saved) => update(app, id, |e| {
            e.base = (saved.size, saved.mtime);
            e.view.state = "uploaded".into();
        }),
        Err(err) => {
            let msg = format!("{err:#}");
            let conflict = msg.starts_with(crate::remote_edit::CHANGED_PREFIX);
            update(app, id, |e| {
                e.view.state = if conflict { "conflict" } else { "error" }.into();
                e.view.message = Some(msg);
            });
        }
    }
}

pub fn list(app: &AppHandle) -> Vec<LocalEditView> {
    app.state::<AppState>()
        .local_edits
        .lock()
        .values()
        .map(|e| e.view.clone())
        .collect()
}

/// Stop watching and delete the temp copy.
pub fn stop(app: &AppHandle, id: &str) {
    if let Some(e) = app.state::<AppState>().local_edits.lock().remove(id) {
        if let Some(t) = e.task {
            t.abort();
        }
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}

/// Stop everything (on quit).
pub fn stop_all(app: &AppHandle) {
    let ids: Vec<String> = app
        .state::<AppState>()
        .local_edits
        .lock()
        .keys()
        .cloned()
        .collect();
    for id in ids {
        stop(app, &id);
    }
    clean_leftovers();
}
