//! Terminal sessions: local PTY, SSH shell, Docker exec. Output is streamed to
//! the webview over a Tauri Channel; input/resize come back via commands.

use crate::state::AppState;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use bollard::Docker;
use futures_util::{FutureExt, StreamExt};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use russh::client::Msg;
use russh::ChannelWriteHalf;
use serde::Serialize;
use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;

/// Output is gathered into chunks of up to this size before being sent to the
/// UI, so a flood of small packets doesn't become a flood of events.
const BATCH: usize = 64 * 1024;

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TermEvent {
    /// base64-encoded bytes
    Data {
        data: String,
    },
    Exit {
        code: Option<u32>,
    },
    Error {
        message: String,
    },
    /// The connection dropped (network, sleep, server restart): the session
    /// did not end on its own, so the UI may reconnect.
    Lost {
        reason: String,
    },
}

pub fn data_event(bytes: &[u8]) -> TermEvent {
    TermEvent::Data {
        data: B64.encode(bytes),
    }
}

pub enum TermWriter {
    Local {
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        killer: Box<dyn ChildKiller + Send + Sync>,
    },
    Ssh {
        chan: ChannelWriteHalf<Msg>,
    },
    Docker {
        input: Pin<Box<dyn AsyncWrite + Send>>,
        docker: Docker,
        exec_id: String,
    },
}

pub struct TermHandle {
    pub writer: tokio::sync::Mutex<TermWriter>,
    /// Set while the UI is behind on drawing output; readers stop reading,
    /// so the remote side is slowed down instead of memory filling up.
    pub paused: watch::Sender<bool>,
}

/// Wait while output is paused (async readers).
async fn wait_unpaused(rx: &mut watch::Receiver<bool>) {
    while *rx.borrow() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}

impl TermHandle {
    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().await;
        match &mut *w {
            TermWriter::Local { writer, .. } => {
                writer.write_all(data)?;
                writer.flush()?;
            }
            TermWriter::Ssh { chan } => chan.data(data).await?,
            TermWriter::Docker { input, .. } => {
                input.write_all(data).await?;
                input.flush().await?;
            }
        }
        Ok(())
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let w = self.writer.lock().await;
        match &*w {
            TermWriter::Local { master, .. } => {
                master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })?;
            }
            TermWriter::Ssh { chan } => chan.window_change(cols as u32, rows as u32, 0, 0).await?,
            TermWriter::Docker {
                docker, exec_id, ..
            } => {
                let opts = bollard::query_parameters::ResizeExecOptionsBuilder::new()
                    .h(rows as i32)
                    .w(cols as i32)
                    .build();
                docker.resize_exec(exec_id, opts).await?;
            }
        }
        Ok(())
    }

    pub async fn close(&self) {
        let mut w = self.writer.lock().await;
        match &mut *w {
            TermWriter::Local { killer, .. } => {
                let _ = killer.kill();
            }
            TermWriter::Ssh { chan } => {
                let _ = chan.eof().await;
                let _ = chan.close().await;
            }
            TermWriter::Docker { input, .. } => {
                let _ = input.shutdown().await;
            }
        }
    }
}

fn register(app: &AppHandle, w: TermWriter) -> (String, watch::Receiver<bool>) {
    let id = uuid::Uuid::new_v4().to_string();
    let (paused, rx) = watch::channel(false);
    app.state::<AppState>().terms.lock().insert(
        id.clone(),
        Arc::new(TermHandle {
            writer: tokio::sync::Mutex::new(w),
            paused,
        }),
    );
    (id, rx)
}

fn unregister(app: &AppHandle, id: &str) {
    app.state::<AppState>().terms.lock().remove(id);
}

pub fn get(app: &AppHandle, id: &str) -> Result<Arc<TermHandle>> {
    app.state::<AppState>()
        .terms
        .lock()
        .get(id)
        .cloned()
        .ok_or_else(|| anyhow!("terminal session not found"))
}

/// Spawn the user's shell in a local PTY.
pub fn open_local(
    app: AppHandle,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    out: Channel<TermEvent>,
) -> Result<String> {
    let shell = app.state::<AppState>().settings.lock().local_shell.clone();
    let pair = native_pty_system().openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.cwd(
        cwd.map(|c| crate::paths::expand_tilde(&c))
            .unwrap_or_else(crate::paths::home_dir),
    );
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let killer = child.clone_killer();
    let (id, paused) = register(
        &app,
        TermWriter::Local {
            writer,
            master: pair.master,
            killer,
        },
    );

    let id2 = id.clone();
    std::thread::spawn(move || {
        // A large buffer batches whatever the PTY has ready into one event.
        let mut buf = vec![0u8; BATCH];
        loop {
            while *paused.borrow() {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out.send(data_event(&buf[..n])).is_err() {
                        break;
                    }
                }
            }
        }
        let code = child.wait().ok().map(|s| s.exit_code());
        let _ = out.send(TermEvent::Exit { code });
        unregister(&app, &id2);
    });
    Ok(id)
}

/// tmux session name for a terminal tab; only safe characters are kept.
fn tmux_name(key: &str) -> Option<String> {
    let k: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    (!k.is_empty()).then(|| format!("shellport-{k}"))
}

/// Quote a string for a POSIX shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Shell script that attaches to (or creates) the tab's tmux session. The
/// startup command runs only when the session is first created; without tmux
/// it falls back to a normal login shell.
fn tmux_script(name: &str, startup: Option<&str>) -> String {
    let start = startup
        .map(|c| format!(" && tmux send-keys -t {name} {} Enter", sh_quote(c)))
        .unwrap_or_default();
    format!(
        "if command -v tmux >/dev/null 2>&1; then \
           tmux has-session -t {name} 2>/dev/null || {{ tmux new-session -d -s {name}{start}; }}; \
           exec tmux attach-session -t {name}; \
         else echo 'Shellport: tmux is not installed on this server; opened a normal shell.' >&2; exec \"$SHELL\" -l; fi"
    )
}

/// Open an interactive shell on a saved SSH host. With `session_key` and the
/// host's tmux option, the shell lives in a tmux session that survives drops.
pub async fn open_ssh(
    app: AppHandle,
    host_id: String,
    cols: u16,
    rows: u16,
    command: Option<String>,
    session_key: Option<String>,
    out: Channel<TermEvent>,
) -> Result<String> {
    let conn = crate::ssh::get_conn(app.clone(), host_id, 0).await?;
    let ch = conn.handle.channel_open_session().await?;
    ch.request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
        .await?;
    let _ = ch.set_env(false, "COLORTERM", "truecolor").await;
    let startup = conn
        .host
        .startup_command
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let tmux = if conn.host.tmux_session && command.is_none() {
        session_key.as_deref().and_then(tmux_name)
    } else {
        None
    };
    match (&command, &tmux) {
        (Some(c), _) => ch.exec(true, c.as_str()).await?,
        (None, Some(name)) => ch.exec(true, tmux_script(name, startup).as_str()).await?,
        (None, None) => ch.request_shell(true).await?,
    }
    let (mut read, write) = ch.split();
    if command.is_none() && tmux.is_none() {
        if let Some(start) = startup {
            write.data(format!("{start}\n").as_bytes()).await?;
        }
    }
    let (id, mut paused) = register(&app, TermWriter::Ssh { chan: write });
    let id2 = id.clone();
    // Opt-in per host: plain-text log of what this terminal showed.
    let mut log = if conn.host.log_terminal && command.is_none() {
        open_log(&conn.host).await
    } else {
        None
    };
    tokio::spawn(async move {
        let conn = conn; // keep the connection alive while the shell is open
        let mut code = None;
        let mut closed = false;
        let mut lines = crate::activity::LogLines::default();
        'outer: loop {
            wait_unpaused(&mut paused).await;
            let Some(first) = read.wait().await else {
                break;
            };
            // Gather everything already received into one event.
            let mut buf = Vec::new();
            let mut next = Some(first);
            while let Some(msg) = next.take() {
                match msg {
                    russh::ChannelMsg::Data { data }
                    | russh::ChannelMsg::ExtendedData { data, .. } => buf.extend_from_slice(&data),
                    russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                    russh::ChannelMsg::Close => closed = true,
                    _ => {}
                }
                if closed || buf.len() >= BATCH {
                    break;
                }
                next = read.wait().now_or_never().flatten();
            }
            if let Some(f) = log.as_mut() {
                use tokio::io::AsyncWriteExt;
                let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                if f.write_all(&lines.feed(&buf, &stamp)).await.is_err() {
                    log = None;
                }
            }
            if !buf.is_empty() && out.send(data_event(&buf)).is_err() {
                break 'outer;
            }
            if closed {
                break;
            }
        }
        // The channel ended without the server closing it or reporting an exit
        // status: the connection itself is gone.
        if !closed && code.is_none() {
            let reason = if conn.is_closed() {
                "the connection to the server was lost"
            } else {
                "the session ended unexpectedly"
            };
            let _ = out.send(TermEvent::Lost {
                reason: reason.into(),
            });
        } else {
            let _ = out.send(TermEvent::Exit { code });
        }
        unregister(&app, &id2);
    });
    Ok(id)
}

/// Folder holding a host's terminal logs.
pub fn log_dir(host: &crate::store::hosts::Host) -> std::path::PathBuf {
    let safe: String = host
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    crate::paths::config_dir().join("logs").join(format!(
        "{safe}-{}",
        host.id.chars().take(8).collect::<String>()
    ))
}

/// A new private log file for one terminal session (None if it can't be created).
async fn open_log(host: &crate::store::hosts::Host) -> Option<tokio::fs::File> {
    let dir = log_dir(host);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("terminal log folder: {e}");
        return None;
    }
    // One file per session: a new name if two sessions start in the same second.
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let mut path = dir.join(format!("{stamp}.log"));
    for n in 2.. {
        if !path.exists() {
            break;
        }
        path = dir.join(format!("{stamp}-{n}.log"));
    }
    let mut o = tokio::fs::OpenOptions::new();
    o.create_new(true).write(true);
    #[cfg(unix)]
    o.mode(0o600);
    match o.open(&path).await {
        Ok(f) => Some(f),
        Err(e) => {
            log::warn!("terminal log {}: {e}", path.display());
            None
        }
    }
}

/// End the tmux session behind a closed tab (best effort).
pub async fn kill_tmux(app: AppHandle, host_id: String, session_key: String) -> Result<()> {
    let Some(name) = tmux_name(&session_key) else {
        return Ok(());
    };
    let conn = crate::ssh::get_conn(app, host_id, 0).await?;
    conn.exec_capture(&format!("tmux kill-session -t {name} 2>/dev/null; true"))
        .await?;
    Ok(())
}

/// Interactive `docker exec` with a TTY via the Docker API.
pub async fn open_docker_exec(
    app: AppHandle,
    docker: Docker,
    container: String,
    cmd: Vec<String>,
    user: Option<String>,
    cols: u16,
    rows: u16,
    out: Channel<TermEvent>,
) -> Result<String> {
    let cfg = bollard::models::ExecConfig {
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        tty: Some(true),
        cmd: Some(cmd),
        user,
        env: Some(vec!["TERM=xterm-256color".into()]),
        ..Default::default()
    };
    let exec = docker.create_exec(&container, cfg).await?;
    let started = docker
        .start_exec(&exec.id, None::<bollard::exec::StartExecOptions>)
        .await?;
    let bollard::exec::StartExecResults::Attached { mut output, input } = started else {
        return Err(anyhow!("exec started detached"));
    };
    let (id, mut paused) = register(
        &app,
        TermWriter::Docker {
            input,
            docker: docker.clone(),
            exec_id: exec.id.clone(),
        },
    );
    let opts = bollard::query_parameters::ResizeExecOptionsBuilder::new()
        .h(rows as i32)
        .w(cols as i32)
        .build();
    let _ = docker.resize_exec(&exec.id, opts).await;
    let id2 = id.clone();
    tokio::spawn(async move {
        'outer: loop {
            wait_unpaused(&mut paused).await;
            let Some(chunk) = output.next().await else {
                break;
            };
            let mut buf = Vec::new();
            let mut next = Some(chunk);
            while let Some(chunk) = next.take() {
                match chunk {
                    Ok(o) => buf.extend_from_slice(&o.into_bytes()),
                    Err(e) => {
                        let _ = out.send(data_event(&buf));
                        let _ = out.send(TermEvent::Error {
                            message: e.to_string(),
                        });
                        break 'outer;
                    }
                }
                if buf.len() >= BATCH {
                    break;
                }
                next = output.next().now_or_never().flatten();
            }
            if out.send(data_event(&buf)).is_err() {
                break;
            }
        }
        let code = docker
            .inspect_exec(&exec.id)
            .await
            .ok()
            .and_then(|i| i.exit_code)
            .map(|c| c as u32);
        let _ = out.send(TermEvent::Exit { code });
        unregister(&app, &id2);
    });
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_names_are_safe() {
        assert_eq!(tmux_name("ab12-x_Y").as_deref(), Some("shellport-ab12-x_Y"));
        assert_eq!(tmux_name("a;rm -rf /").as_deref(), Some("shellport-arm-rf"));
        assert_eq!(tmux_name("$();"), None);
    }

    #[test]
    fn startup_command_is_quoted() {
        let s = tmux_script("shellport-t1", Some("echo 'hi' && cd /srv"));
        assert!(
            s.contains(r#"send-keys -t shellport-t1 'echo '\''hi'\'' && cd /srv' Enter"#),
            "{s}"
        );
        assert!(!tmux_script("shellport-t1", None).contains("send-keys"));
    }
}
