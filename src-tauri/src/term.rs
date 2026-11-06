//! Terminal sessions: local PTY, SSH shell, Docker exec. Output is streamed to
//! the webview over a Tauri Channel; input/resize come back via commands.

use crate::state::AppState;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use bollard::Docker;
use futures_util::StreamExt;
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

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TermEvent {
    /// base64-encoded bytes
    Data { data: String },
    Exit { code: Option<u32> },
    Error { message: String },
}

pub fn data_event(bytes: &[u8]) -> TermEvent {
    TermEvent::Data { data: B64.encode(bytes) }
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
                master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
            }
            TermWriter::Ssh { chan } => chan.window_change(cols as u32, rows as u32, 0, 0).await?,
            TermWriter::Docker { docker, exec_id, .. } => {
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

fn register(app: &AppHandle, w: TermWriter) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    app.state::<AppState>()
        .terms
        .lock()
        .insert(id.clone(), Arc::new(TermHandle { writer: tokio::sync::Mutex::new(w) }));
    id
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
pub fn open_local(app: AppHandle, cols: u16, rows: u16, cwd: Option<String>, out: Channel<TermEvent>) -> Result<String> {
    let shell = app.state::<AppState>().settings.lock().local_shell.clone();
    let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.cwd(cwd.map(|c| crate::paths::expand_tilde(&c)).unwrap_or_else(crate::paths::home_dir));
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let killer = child.clone_killer();
    let id = register(&app, TermWriter::Local { writer, master: pair.master, killer });

    let id2 = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
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

/// Open an interactive shell on a saved SSH host.
pub async fn open_ssh(app: AppHandle, host_id: String, cols: u16, rows: u16, command: Option<String>, out: Channel<TermEvent>) -> Result<String> {
    let conn = crate::ssh::get_conn(app.clone(), host_id, 0).await?;
    let ch = conn.handle.channel_open_session().await?;
    ch.request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[]).await?;
    let _ = ch.set_env(false, "COLORTERM", "truecolor").await;
    match &command {
        Some(c) => ch.exec(true, c.as_str()).await?,
        None => ch.request_shell(true).await?,
    }
    let (mut read, write) = ch.split();
    if command.is_none() {
        if let Some(start) = conn.host.startup_command.as_deref().filter(|s| !s.trim().is_empty()) {
            write.data(format!("{start}\n").as_bytes()).await?;
        }
    }
    let id = register(&app, TermWriter::Ssh { chan: write });
    let id2 = id.clone();
    tokio::spawn(async move {
        let _conn = conn; // keep the connection alive while the shell is open
        let mut code = None;
        while let Some(msg) = read.wait().await {
            match msg {
                russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                    if out.send(data_event(&data)).is_err() {
                        break;
                    }
                }
                russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        let _ = out.send(TermEvent::Exit { code });
        unregister(&app, &id2);
    });
    Ok(id)
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
    let started = docker.start_exec(&exec.id, None::<bollard::exec::StartExecOptions>).await?;
    let bollard::exec::StartExecResults::Attached { mut output, input } = started else {
        return Err(anyhow!("exec started detached"));
    };
    let id = register(&app, TermWriter::Docker { input, docker: docker.clone(), exec_id: exec.id.clone() });
    let opts = bollard::query_parameters::ResizeExecOptionsBuilder::new().h(rows as i32).w(cols as i32).build();
    let _ = docker.resize_exec(&exec.id, opts).await;
    let id2 = id.clone();
    tokio::spawn(async move {
        while let Some(chunk) = output.next().await {
            match chunk {
                Ok(o) => {
                    if out.send(data_event(&o.into_bytes())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = out.send(TermEvent::Error { message: e.to_string() });
                    break;
                }
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
