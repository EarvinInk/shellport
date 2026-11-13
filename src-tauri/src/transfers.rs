//! One app-wide transfer queue (M3.1): uploads, downloads and server-to-server
//! copies, with pause, resume and retry, speed and time left. Every SFTP tab
//! and the Transfers panel show the same queue.
//!
//! A transfer is planned once (the list of files and their sizes); after that,
//! each file remembers how many bytes reached the destination, so resume and
//! retry continue where they stopped. A file whose source changed size or
//! modified time since then starts over.

use crate::sftp::{check_no_symlinks, join, safe_component, CHUNK, MAX_DEPTH, MAX_FILES};
use crate::state::AppState;
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};

/// Event carrying one transfer's new state.
pub const EVENT: &str = "transfer-update";
/// At most this many transfers run at once…
const MAX_RUNNING: usize = 4;
/// …and at most this many per host.
const MAX_PER_HOST: usize = 2;

/// One end of a transfer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Endpoint {
    Local {
        path: String,
    },
    #[serde(rename_all = "camelCase")]
    Remote {
        host_id: String,
        path: String,
        /// Through the root session (M3.3).
        #[serde(default)]
        sudo: bool,
    },
}

impl Endpoint {
    fn host(&self) -> Option<&str> {
        match self {
            Endpoint::Remote { host_id, .. } => Some(host_id),
            Endpoint::Local { .. } => None,
        }
    }
    fn path(&self) -> &str {
        match self {
            Endpoint::Local { path } | Endpoint::Remote { path, .. } => path,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TState {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl TState {
    fn finished(self) -> bool {
        matches!(self, TState::Done | TState::Failed | TState::Cancelled)
    }
}

/// What the UI sees of a transfer.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    /// File or folder name being moved.
    pub name: String,
    pub source: Endpoint,
    pub dest_dir: Endpoint,
    pub state: TState,
    pub bytes: u64,
    pub total: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub current: Option<String>,
    /// Bytes per second over the last few seconds (0 when not running).
    pub speed: u64,
    pub eta_secs: Option<u64>,
    pub error: Option<String>,
    /// Set when nothing was written because this many local files would be overwritten.
    pub would_overwrite: Option<u32>,
}

/// One file of a transfer, and how far it got.
#[derive(Clone, Debug, PartialEq)]
struct FileJob {
    src: String,
    dst: String,
    size: u64,
    mtime: Option<u64>,
    done: u64,
    finished: bool,
}

/// Files and folders to create, worked out once per transfer.
#[derive(Clone, Debug, Default)]
struct Plan {
    files: Vec<FileJob>,
    /// Destination folders, parents first.
    dirs: Vec<String>,
}

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

struct Entry {
    view: TransferView,
    overwrite: bool,
    plan: Option<Plan>,
    control: Arc<AtomicU8>,
    handle: Option<tokio::task::AbortHandle>,
    /// "Start now": ignore the queue limits.
    force: bool,
    started: Option<Instant>,
    samples: VecDeque<(Instant, u64)>,
    last_emit: Instant,
}

#[derive(Default)]
pub struct Queue {
    entries: Mutex<Vec<Entry>>,
}

/// Why a transfer stopped early (carried inside an `anyhow::Error`).
#[derive(Debug, thiserror::Error)]
enum Stopped {
    #[error("paused")]
    Paused,
    #[error("cancelled")]
    Cancelled,
    #[error("{0} local file(s) would be overwritten")]
    Exists(u32),
}

fn emit(app: &AppHandle, v: TransferView) {
    let _ = app.emit(EVENT, v);
}

fn name_of(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .find(|n| !n.is_empty())
        .unwrap_or("transfer")
        .to_string()
}

impl Queue {
    pub fn list(&self) -> Vec<TransferView> {
        self.entries.lock().iter().map(|e| e.view.clone()).collect()
    }

    fn with<R>(&self, id: &str, f: impl FnOnce(&mut Entry) -> R) -> Option<R> {
        self.entries
            .lock()
            .iter_mut()
            .find(|e| e.view.id == id)
            .map(f)
    }
}

/// Queue a transfer; it starts as soon as the limits allow.
pub fn add(app: &AppHandle, source: Endpoint, dest_dir: Endpoint, overwrite: bool) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let view = TransferView {
        id: id.clone(),
        name: name_of(source.path()),
        source,
        dest_dir,
        state: TState::Queued,
        bytes: 0,
        total: 0,
        files_done: 0,
        files_total: 0,
        current: None,
        speed: 0,
        eta_secs: None,
        error: None,
        would_overwrite: None,
    };
    let q = &app.state::<AppState>().transfers;
    q.entries.lock().push(Entry {
        view: view.clone(),
        overwrite,
        plan: None,
        control: Arc::new(AtomicU8::new(RUN)),
        handle: None,
        force: false,
        started: None,
        samples: VecDeque::new(),
        last_emit: Instant::now(),
    });
    emit(app, view);
    pump(app);
    id
}

/// Which queued transfers may start now, in queue order.
fn startable(entries: &[(TState, bool, Vec<String>)]) -> Vec<usize> {
    let mut running = 0;
    let mut per_host: HashMap<&str, usize> = HashMap::new();
    for (s, _, hosts) in entries {
        if *s == TState::Running {
            running += 1;
            for h in hosts {
                *per_host.entry(h).or_default() += 1;
            }
        }
    }
    let mut out = Vec::new();
    for (i, (s, force, hosts)) in entries.iter().enumerate() {
        if *s != TState::Queued {
            continue;
        }
        let fits = running < MAX_RUNNING
            && hosts
                .iter()
                .all(|h| per_host.get(h.as_str()).copied().unwrap_or(0) < MAX_PER_HOST);
        if *force || fits {
            running += 1;
            for h in hosts {
                *per_host.entry(h).or_default() += 1;
            }
            out.push(i);
        }
    }
    out
}

fn hosts_of(v: &TransferView) -> Vec<String> {
    let mut h: Vec<String> = [v.source.host(), v.dest_dir.host()]
        .into_iter()
        .flatten()
        .map(String::from)
        .collect();
    h.dedup();
    h
}

/// Start whatever the limits allow.
fn pump(app: &AppHandle) {
    let q = &app.state::<AppState>().transfers;
    let mut started = Vec::new();
    {
        let mut entries = q.entries.lock();
        let shape: Vec<_> = entries
            .iter()
            .map(|e| (e.view.state, e.force, hosts_of(&e.view)))
            .collect();
        for i in startable(&shape) {
            let e = &mut entries[i];
            e.view.state = TState::Running;
            e.view.error = None;
            e.view.would_overwrite = None;
            e.control.store(RUN, Ordering::SeqCst);
            e.samples.clear();
            e.started.get_or_insert_with(Instant::now);
            let (a, id) = (app.clone(), e.view.id.clone());
            let h = tokio::spawn(async move {
                let res = execute(&a, &id).await;
                finish(&a, &id, res);
            });
            e.handle = Some(h.abort_handle());
            started.push(e.view.clone());
        }
    }
    for v in started {
        emit(app, v);
    }
}

/// Record how a run ended (only once: a later call for the same run does nothing).
fn finish(app: &AppHandle, id: &str, res: Result<()>) {
    let state = app.state::<AppState>();
    let Some((view, record, partial)) = state
        .transfers
        .with(id, |e| {
            if e.view.state != TState::Running {
                return None;
            }
            e.handle = None;
            e.view.speed = 0;
            e.view.eta_secs = None;
            e.view.current = None;
            let stop = res
                .as_ref()
                .err()
                .and_then(|err| err.downcast_ref::<Stopped>());
            let mut partial = None;
            let record = match (&res, stop) {
                (Ok(()), _) => {
                    e.view.state = TState::Done;
                    true
                }
                (_, Some(Stopped::Paused)) => {
                    e.view.state = TState::Paused;
                    false
                }
                (_, Some(Stopped::Cancelled)) => {
                    e.view.state = TState::Cancelled;
                    partial = in_progress(e);
                    true
                }
                (_, Some(Stopped::Exists(n))) => {
                    // Nothing written yet: the UI offers Overwrite, which retries.
                    e.view.state = TState::Failed;
                    e.view.would_overwrite = Some(*n);
                    e.view.error = Some(format!(
                        "{n} file{} in the destination would be overwritten",
                        if *n == 1 { "" } else { "s" }
                    ));
                    false
                }
                (Err(err), None) => {
                    e.view.state = TState::Failed;
                    e.view.error = Some(format!("{err:#}"));
                    true
                }
            };
            Some((
                e.view.clone(),
                record.then(|| e.started.map(|s| s.elapsed())).flatten(),
                partial,
            ))
        })
        .flatten()
    else {
        return;
    };
    if let Some((dest, path)) = partial {
        let a = app.clone();
        tokio::spawn(async move { remove_partial(&a, &dest, &path).await });
    }
    if let Some(took) = record {
        record_activity(app, &view, took);
    }
    emit(app, view);
    pump(app);
}

/// The destination file a stopped transfer was part-way through, if any.
fn in_progress(e: &Entry) -> Option<(Endpoint, String)> {
    let f = e
        .plan
        .as_ref()?
        .files
        .iter()
        .find(|f| !f.finished && f.done > 0)?;
    Some((e.view.dest_dir.clone(), f.dst.clone()))
}

async fn remove_partial(app: &AppHandle, dest: &Endpoint, path: &str) {
    match dest {
        Endpoint::Local { .. } => {
            let _ = tokio::fs::remove_file(path).await;
        }
        Endpoint::Remote { host_id, sudo, .. } => {
            if let Ok(s) = crate::sftp::session_as(app, host_id, *sudo).await {
                let _ = s.remove_file(path).await;
            }
        }
    }
}

fn record_activity(app: &AppHandle, v: &TransferView, took: Duration) {
    let state = app.state::<AppState>();
    let host_name = |id: &str| {
        state
            .hosts
            .lock()
            .get(id)
            .map(|h| h.name)
            .unwrap_or_else(|_| "?".into())
    };
    let (title, what, host) = match (&v.source, &v.dest_dir) {
        (Endpoint::Remote { host_id, path, .. }, Endpoint::Local { path: d }) => (
            host_name(host_id),
            format!("Download {path} → {d}"),
            host_id.clone(),
        ),
        (
            Endpoint::Local { path },
            Endpoint::Remote {
                host_id, path: d, ..
            },
        ) => (
            host_name(host_id),
            format!("Upload {path} → {d}"),
            host_id.clone(),
        ),
        (
            Endpoint::Remote {
                host_id: a, path, ..
            },
            Endpoint::Remote {
                host_id: b,
                path: d,
                ..
            },
        ) => (
            format!("{} → {}", host_name(a), host_name(b)),
            format!("Copy {}:{path} → {}:{d}", host_name(a), host_name(b)),
            a.clone(),
        ),
        (Endpoint::Local { path }, Endpoint::Local { path: d }) => {
            ("local".into(), format!("Copy {path} → {d}"), String::new())
        }
    };
    let ok = v.state == TState::Done;
    let mut e = crate::activity::ActivityEntry::new(
        crate::activity::ActivityKind::Transfer,
        title,
        &what,
        ok,
    );
    e.host_id = (!host.is_empty()).then_some(host);
    e.message = match v.state {
        TState::Cancelled => Some("cancelled".into()),
        TState::Failed => v.error.clone(),
        _ => None,
    };
    e.duration_ms = Some(took.as_millis() as u64);
    state.activity.add(e);
}

/// Ask a running transfer to stop at the next chunk; force it after a few seconds
/// (a hung connection never reaches the next chunk).
fn stop(app: &AppHandle, id: &str, how: u8) {
    let q = &app.state::<AppState>().transfers;
    let Some(state) = q.with(id, |e| e.view.state) else {
        return;
    };
    match state {
        TState::Running => {
            q.with(id, |e| e.control.store(how, Ordering::SeqCst));
            let (a, id) = (app.clone(), id.to_string());
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let h = a
                    .state::<AppState>()
                    .transfers
                    .with(&id, |e| {
                        (e.view.state == TState::Running)
                            .then(|| e.handle.clone())
                            .flatten()
                    })
                    .flatten();
                if let Some(h) = h {
                    h.abort();
                    let why = if how == PAUSE {
                        Stopped::Paused
                    } else {
                        Stopped::Cancelled
                    };
                    finish(&a, &id, Err(why.into()));
                }
            });
        }
        TState::Queued | TState::Paused | TState::Failed if how == CANCEL => {
            let partial = q.with(id, |e| {
                e.view.state = TState::Cancelled;
                e.view.would_overwrite = None;
                (in_progress(e), e.view.clone())
            });
            if let Some((partial, view)) = partial {
                if let Some((dest, path)) = partial {
                    let a = app.clone();
                    tokio::spawn(async move { remove_partial(&a, &dest, &path).await });
                }
                emit(app, view);
            }
        }
        TState::Queued if how == PAUSE => {
            if let Some(v) = q.with(id, |e| {
                e.view.state = TState::Paused;
                e.view.clone()
            }) {
                emit(app, v);
            }
        }
        _ => {}
    }
}

pub fn pause(app: &AppHandle, id: &str) {
    stop(app, id, PAUSE);
}

pub fn cancel(app: &AppHandle, id: &str) {
    stop(app, id, CANCEL);
}

/// Resume a paused transfer or retry a failed one (optionally allowing overwrites).
pub fn resume(app: &AppHandle, id: &str, overwrite: bool) {
    let q = &app.state::<AppState>().transfers;
    let v = q.with(id, |e| {
        if !matches!(e.view.state, TState::Paused | TState::Failed) {
            return None;
        }
        e.view.state = TState::Queued;
        e.view.error = None;
        e.view.would_overwrite = None;
        e.overwrite |= overwrite;
        Some(e.view.clone())
    });
    if let Some(Some(v)) = v {
        emit(app, v);
        pump(app);
    }
}

/// Start a queued transfer now, past the queue limits.
pub fn start_now(app: &AppHandle, id: &str) {
    app.state::<AppState>()
        .transfers
        .with(id, |e| e.force = true);
    pump(app);
}

/// Drop finished transfers from the list (all of them, or the given ones).
pub fn clear(app: &AppHandle, ids: Option<Vec<String>>) {
    app.state::<AppState>()
        .transfers
        .entries
        .lock()
        .retain(|e| {
            !(e.view.state.finished() && ids.as_ref().is_none_or(|ids| ids.contains(&e.view.id)))
        });
}

// ---------------------------------------------------------------------------
// Doing the work

async fn session_for(app: &AppHandle, ep: &Endpoint) -> Result<Option<Arc<SftpSession>>> {
    Ok(match ep {
        Endpoint::Remote { host_id, sudo, .. } => {
            Some(crate::sftp::session_as(app, host_id, *sudo).await?)
        }
        Endpoint::Local { .. } => None,
    })
}

async fn execute(app: &AppHandle, id: &str) -> Result<()> {
    let q = &app.state::<AppState>().transfers;
    let (source, dest_dir, overwrite, plan, control) = q
        .with(id, |e| {
            (
                e.view.source.clone(),
                e.view.dest_dir.clone(),
                e.overwrite,
                e.plan.clone(),
                e.control.clone(),
            )
        })
        .context("transfer not found")?;
    if let (
        Endpoint::Remote {
            host_id: a,
            path: sp,
            ..
        },
        Endpoint::Remote {
            host_id: b,
            path: dd,
            ..
        },
    ) = (&source, &dest_dir)
    {
        if a == b && join(dd, &name_of(sp)) == sp.trim_end_matches('/') {
            bail!("source and destination are the same");
        }
    }
    let src_s = session_for(app, &source).await?;
    let dst_s = session_for(app, &dest_dir).await?;

    let mut plan = match plan {
        Some(mut p) => {
            refresh_offsets(&mut p, src_s.as_deref(), dst_s.as_deref()).await;
            p
        }
        None => {
            let p = make_plan(&source, &dest_dir, src_s.as_deref()).await?;
            if !overwrite {
                let n = count_existing(&p, src_s.as_deref(), dst_s.as_deref()).await;
                if n > 0 {
                    return Err(Stopped::Exists(n).into());
                }
            }
            p
        }
    };
    // Publish the plan (sizes, file count) before copying.
    let total: u64 = plan.files.iter().map(|f| f.size).sum();
    let view = q.with(id, |e| {
        e.view.total = total;
        e.view.files_total = plan.files.len() as u32;
        e.view.bytes = plan.files.iter().map(|f| f.done).sum();
        e.view.files_done = plan.files.iter().filter(|f| f.finished).count() as u32;
        e.plan = Some(plan.clone());
        e.view.clone()
    });
    if let Some(v) = view {
        emit(app, v);
    }

    make_dirs(&plan, &dest_dir, dst_s.as_deref()).await?;
    let local_base = match &dest_dir {
        Endpoint::Local { path } => Some(crate::paths::expand_tilde(path)),
        Endpoint::Remote { .. } => None,
    };
    let mut buf = vec![0u8; CHUNK];
    for i in 0..plan.files.len() {
        if plan.files[i].finished {
            continue;
        }
        let job = plan.files[i].clone();
        let mut reader = open_src(&job, src_s.as_deref())
            .await
            .with_context(|| format!("reading {}", job.src))?;
        let mut writer = open_dst(&job, dst_s.as_deref(), local_base.as_deref())
            .await
            .with_context(|| format!("writing {}", job.dst))?;
        let mut done = job.done;
        loop {
            match control.load(Ordering::SeqCst) {
                PAUSE => {
                    writer.shutdown().await.ok();
                    return Err(Stopped::Paused.into());
                }
                CANCEL => {
                    writer.shutdown().await.ok();
                    return Err(Stopped::Cancelled.into());
                }
                _ => {}
            }
            let n = reader
                .read(&mut buf)
                .await
                .with_context(|| format!("reading {}", job.src))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .await
                .with_context(|| format!("writing {}", job.dst))?;
            done += n as u64;
            plan.files[i].done = done;
            progress(app, id, i, done, false, &job.src);
        }
        writer
            .shutdown()
            .await
            .with_context(|| format!("finishing {}", job.dst))?;
        copy_mode(&job, src_s.as_deref(), dst_s.as_deref()).await;
        plan.files[i].finished = true;
        progress(app, id, i, done, true, &job.src);
    }
    Ok(())
}

/// Note progress on one file; emits at most every 150 ms unless `force`.
fn progress(app: &AppHandle, id: &str, file: usize, done: u64, finished: bool, name: &str) {
    let v = app.state::<AppState>().transfers.with(id, |e| {
        let plan = e.plan.as_mut()?;
        let f = plan.files.get_mut(file)?;
        f.done = done;
        f.finished |= finished;
        let bytes: u64 = plan.files.iter().map(|f| f.done).sum();
        e.view.bytes = bytes;
        e.view.files_done = plan.files.iter().filter(|f| f.finished).count() as u32;
        e.view.current = Some(name.to_string());
        let now = Instant::now();
        e.samples.push_back((now, bytes));
        while e.samples.len() > 2 && now.duration_since(e.samples[0].0) > Duration::from_secs(5) {
            e.samples.pop_front();
        }
        e.view.speed = speed(&e.samples);
        e.view.eta_secs =
            (e.view.speed > 0).then(|| e.view.total.saturating_sub(bytes) / e.view.speed);
        if finished || now.duration_since(e.last_emit) > Duration::from_millis(150) {
            e.last_emit = now;
            return Some(e.view.clone());
        }
        None
    });
    if let Some(Some(v)) = v {
        emit(app, v);
    }
}

/// Bytes per second between the oldest and newest sample.
fn speed(samples: &VecDeque<(Instant, u64)>) -> u64 {
    let (Some(a), Some(b)) = (samples.front(), samples.back()) else {
        return 0;
    };
    let dt = b.0.duration_since(a.0).as_secs_f64();
    if dt < 0.5 {
        return 0;
    }
    (b.1.saturating_sub(a.1) as f64 / dt) as u64
}

/// Where a resumed file continues: from what reached the destination, or from
/// the start if the source changed since it was planned.
fn resume_offset(job: &FileJob, src_now: Option<(u64, Option<u64>)>, dst_len: Option<u64>) -> u64 {
    match src_now {
        Some((size, mtime)) if size == job.size && mtime == job.mtime => {
            job.done.min(dst_len.unwrap_or(0))
        }
        _ => 0,
    }
}

async fn stat(path: &str, s: Option<&SftpSession>) -> Option<(u64, Option<u64>)> {
    match s {
        Some(s) => s
            .metadata(path)
            .await
            .ok()
            .map(|m| (m.size.unwrap_or(0), m.mtime.map(u64::from))),
        None => std::fs::metadata(path)
            .ok()
            .map(|m| (m.len(), mtime_of(&m))),
    }
}

fn mtime_of(m: &std::fs::Metadata) -> Option<u64> {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

async fn refresh_offsets(plan: &mut Plan, src: Option<&SftpSession>, dst: Option<&SftpSession>) {
    for f in plan.files.iter_mut().filter(|f| !f.finished && f.done > 0) {
        let src_now = stat(&f.src, src).await;
        let dst_len = stat(&f.dst, dst).await.map(|d| d.0);
        f.done = resume_offset(f, src_now, dst_len);
    }
}

/// List what to copy: every file under the source, with its destination path.
async fn make_plan(
    source: &Endpoint,
    dest_dir: &Endpoint,
    src: Option<&SftpSession>,
) -> Result<Plan> {
    let name = name_of(source.path());
    safe_component(&name)?;
    let dest_root = match dest_dir {
        Endpoint::Local { path } => crate::paths::expand_tilde(path)
            .join(&name)
            .to_string_lossy()
            .into_owned(),
        Endpoint::Remote { path, .. } => join(path, &name),
    };
    let local_dest = matches!(dest_dir, Endpoint::Local { .. });
    let dst_join = |base: &str, n: &str| {
        if local_dest {
            PathBuf::from(base).join(n).to_string_lossy().into_owned()
        } else {
            join(base, n)
        }
    };
    let mut plan = Plan::default();
    // (source path, destination path, depth)
    let mut stack = vec![(source.path().to_string(), dest_root, 0usize)];
    while let Some((sp, dp, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            bail!("folder is nested too deeply (over {MAX_DEPTH} levels)");
        }
        let (is_dir, is_link, size, mtime) = match src {
            Some(s) => {
                // lstat: a symlinked folder is not followed (avoids loops);
                // a symlink to a file copies that file's contents.
                let lm = s
                    .symlink_metadata(sp.as_str())
                    .await
                    .with_context(|| format!("reading {sp}"))?;
                let is_link = lm.file_type().is_symlink();
                let m = if is_link {
                    s.metadata(sp.as_str()).await?
                } else {
                    lm
                };
                (
                    m.is_dir(),
                    is_link,
                    m.size.unwrap_or(0),
                    m.mtime.map(u64::from),
                )
            }
            None => {
                let lm = std::fs::symlink_metadata(&sp).with_context(|| format!("reading {sp}"))?;
                let is_link = lm.file_type().is_symlink();
                let m = if is_link { std::fs::metadata(&sp)? } else { lm };
                (m.is_dir(), is_link, m.len(), mtime_of(&m))
            }
        };
        if is_dir {
            if is_link {
                continue;
            }
            plan.dirs.push(dp.clone());
            let mut names = Vec::new();
            match src {
                Some(s) => {
                    for e in s.read_dir(sp.as_str()).await? {
                        names.push(e.file_name());
                    }
                }
                None => {
                    for e in std::fs::read_dir(&sp)? {
                        names.push(e?.file_name().to_string_lossy().into_owned());
                    }
                }
            }
            names.sort();
            for n in names.into_iter().rev() {
                if n == "." || n == ".." {
                    continue;
                }
                safe_component(&n)?;
                let child_src = if src.is_some() {
                    join(&sp, &n)
                } else {
                    PathBuf::from(&sp).join(&n).to_string_lossy().into_owned()
                };
                stack.push((child_src, dst_join(&dp, &n), depth + 1));
            }
        } else {
            if plan.files.len() >= MAX_FILES {
                bail!("too many files (over {MAX_FILES})");
            }
            plan.files.push(FileJob {
                src: sp,
                dst: dp,
                size,
                mtime,
                done: 0,
                finished: false,
            });
        }
    }
    Ok(plan)
}

/// How many destination files already exist. Checked for downloads and for
/// server-to-server copies (two host entries can be the same server, and copying
/// a file onto itself would empty it); uploads replace remote files, as before.
async fn count_existing(plan: &Plan, src: Option<&SftpSession>, dst: Option<&SftpSession>) -> u32 {
    match (src, dst) {
        (_, None) => plan
            .files
            .iter()
            .filter(|f| std::fs::symlink_metadata(&f.dst).is_ok())
            .count() as u32,
        (Some(_), Some(d)) => {
            let mut n = 0;
            for f in &plan.files {
                if d.try_exists(f.dst.as_str()).await.unwrap_or(false) {
                    n += 1;
                }
            }
            n
        }
        (None, Some(_)) => 0,
    }
}

async fn make_dirs(plan: &Plan, dest_dir: &Endpoint, dst: Option<&SftpSession>) -> Result<()> {
    match (dest_dir, dst) {
        (Endpoint::Remote { .. }, Some(s)) => {
            for d in &plan.dirs {
                if !s.try_exists(d.as_str()).await.unwrap_or(false) {
                    s.create_dir(d.as_str())
                        .await
                        .with_context(|| format!("creating {d}"))?;
                }
            }
        }
        (Endpoint::Local { path }, _) => {
            let base = crate::paths::expand_tilde(path);
            for d in &plan.dirs {
                let d = PathBuf::from(d);
                check_no_symlinks(&base, &d)?;
                tokio::fs::create_dir_all(&d)
                    .await
                    .with_context(|| format!("creating {}", d.display()))?;
            }
        }
        _ => {}
    }
    Ok(())
}

type Reader = Box<dyn AsyncRead + Unpin + Send>;
type Writer = Box<dyn AsyncWrite + Unpin + Send>;

async fn open_src(job: &FileJob, s: Option<&SftpSession>) -> Result<Reader> {
    Ok(match s {
        Some(s) => {
            let mut f = s.open(job.src.as_str()).await?;
            if job.done > 0 {
                f.seek(std::io::SeekFrom::Start(job.done)).await?;
            }
            Box::new(f)
        }
        None => {
            let mut f = tokio::fs::File::open(&job.src).await?;
            if job.done > 0 {
                f.seek(std::io::SeekFrom::Start(job.done)).await?;
            }
            Box::new(f)
        }
    })
}

async fn open_dst(
    job: &FileJob,
    s: Option<&SftpSession>,
    local_base: Option<&std::path::Path>,
) -> Result<Writer> {
    Ok(match s {
        Some(s) => {
            let mut flags = OpenFlags::CREATE | OpenFlags::WRITE;
            if job.done == 0 {
                flags |= OpenFlags::TRUNCATE;
            }
            let mut f = s.open_with_flags(job.dst.as_str(), flags).await?;
            if job.done > 0 {
                // Drop anything past what we know arrived, then append.
                let mut attrs = russh_sftp::protocol::FileAttributes::empty();
                attrs.size = Some(job.done);
                f.set_metadata(attrs).await?;
                f.seek(std::io::SeekFrom::Start(job.done)).await?;
            }
            Box::new(f)
        }
        None => {
            let path = PathBuf::from(&job.dst);
            let base = local_base.ok_or_else(|| anyhow!("no local destination"))?;
            check_no_symlinks(base, &path)?;
            let std_file = crate::sftp::open_local_dst(&path, job.done == 0)?;
            let mut f = tokio::fs::File::from_std(std_file);
            if job.done > 0 {
                f.set_len(job.done).await?;
                f.seek(std::io::SeekFrom::Start(job.done)).await?;
            }
            Box::new(f)
        }
    })
}

/// Keep permission bits on remote destinations (e.g. executables).
async fn copy_mode(job: &FileJob, src: Option<&SftpSession>, dst: Option<&SftpSession>) {
    let Some(dst) = dst else { return };
    let mode = match src {
        Some(s) => s
            .metadata(job.src.as_str())
            .await
            .ok()
            .and_then(|m| m.permissions),
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(&job.src)
                    .ok()
                    .map(|m| m.permissions().mode())
            }
            #[cfg(not(unix))]
            None
        }
    };
    if let Some(mode) = mode {
        let mut attrs = russh_sftp::protocol::FileAttributes::empty();
        attrs.permissions = Some(mode & 0o777);
        let _ = dst.set_metadata(job.dst.as_str(), attrs).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(size: u64, mtime: Option<u64>, done: u64) -> FileJob {
        FileJob {
            src: "s".into(),
            dst: "d".into(),
            size,
            mtime,
            done,
            finished: false,
        }
    }

    #[test]
    fn resume_continues_or_starts_over() {
        let j = job(1000, Some(50), 600);
        // Unchanged source: continue from what reached the destination.
        assert_eq!(resume_offset(&j, Some((1000, Some(50))), Some(600)), 600);
        // The destination holds less than we counted (a write was cut off).
        assert_eq!(resume_offset(&j, Some((1000, Some(50))), Some(400)), 400);
        // The destination is gone.
        assert_eq!(resume_offset(&j, Some((1000, Some(50))), None), 0);
        // The source changed size or modified time, or vanished: start over.
        assert_eq!(resume_offset(&j, Some((1200, Some(50))), Some(600)), 0);
        assert_eq!(resume_offset(&j, Some((1000, Some(51))), Some(600)), 0);
        assert_eq!(resume_offset(&j, None, Some(600)), 0);
    }

    #[test]
    fn queue_limits() {
        let h = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let q = |s: TState, f: bool, hs: &[&str]| (s, f, h(hs));
        // Two per host: the third transfer on web waits; db still starts.
        let e = vec![
            q(TState::Queued, false, &["web"]),
            q(TState::Queued, false, &["web"]),
            q(TState::Queued, false, &["web"]),
            q(TState::Queued, false, &["db"]),
        ];
        assert_eq!(startable(&e), vec![0, 1, 3]);
        // Four in total, counting ones already running.
        let e = vec![
            q(TState::Running, false, &["a"]),
            q(TState::Running, false, &["b"]),
            q(TState::Running, false, &["c"]),
            q(TState::Queued, false, &["d"]),
            q(TState::Queued, false, &["e"]),
        ];
        assert_eq!(startable(&e), vec![3]);
        // A server-to-server copy counts against both hosts.
        let e = vec![
            q(TState::Running, false, &["a"]),
            q(TState::Running, false, &["a"]),
            q(TState::Queued, false, &["a", "b"]),
        ];
        assert!(startable(&e).is_empty());
        // "Start now" ignores the limits; paused and finished ones never start.
        let e = vec![
            q(TState::Running, false, &["a"]),
            q(TState::Running, false, &["a"]),
            q(TState::Queued, true, &["a"]),
            q(TState::Paused, false, &["z"]),
        ];
        assert_eq!(startable(&e), vec![2]);
    }

    #[test]
    fn speed_needs_half_a_second() {
        let t = Instant::now();
        let mut s = VecDeque::new();
        s.push_back((t, 0));
        s.push_back((t + Duration::from_millis(100), 1000));
        assert_eq!(speed(&s), 0);
        s.push_back((t + Duration::from_secs(2), 4_000_000));
        assert_eq!(speed(&s), 2_000_000);
    }

    #[tokio::test]
    async fn local_plan_and_resume_copy() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("proj");
        std::fs::create_dir_all(src.join("sub/empty")).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::write(src.join("sub/b.bin"), vec![7u8; 3000]).unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let plan = make_plan(
            &Endpoint::Local {
                path: src.to_string_lossy().into(),
            },
            &Endpoint::Local {
                path: out.to_string_lossy().into(),
            },
            None,
        )
        .await
        .unwrap();
        let rel: Vec<_> = plan
            .files
            .iter()
            .map(|f| {
                f.dst
                    .strip_prefix(out.to_str().unwrap())
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(rel, vec!["/proj/a.txt", "/proj/sub/b.bin"]);
        assert!(plan.dirs.iter().any(|d| d.ends_with("/proj/sub/empty")));
        // Resume a half-written file: the first half stays, the rest is appended.
        let mut j = plan.files[1].clone();
        std::fs::create_dir_all(out.join("proj/sub")).unwrap();
        std::fs::write(&j.dst, vec![7u8; 1000]).unwrap();
        j.done = 1000;
        let mut r = open_src(&j, None).await.unwrap();
        let mut w = open_dst(&j, None, Some(&out)).await.unwrap();
        tokio::io::copy(&mut r, &mut w).await.unwrap();
        w.shutdown().await.unwrap();
        assert_eq!(std::fs::read(&j.dst).unwrap(), vec![7u8; 3000]);
        // A destination that already exists is counted before anything is written.
        assert_eq!(count_existing(&plan, None, None).await, 1);
    }
}
