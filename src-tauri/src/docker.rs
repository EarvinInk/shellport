//! Docker management for the local daemon and remote hosts.
//!
//! Remote hosts are reached by tunnelling the remote Docker unix socket over
//! SSH (`direct-streamlocal@openssh.com`) to a private local socket that
//! bollard talks to. If the server forbids stream-local forwarding, we fall
//! back to running the `docker` CLI over an SSH exec channel.

use crate::ssh::SshConn;
use crate::state::AppState;
use crate::term::{data_event, TermEvent};
use anyhow::{anyhow, bail, Context, Result};
use bollard::query_parameters::{
    ListContainersOptionsBuilder, ListImagesOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder,
    RestartContainerOptionsBuilder, StopContainerOptionsBuilder,
};
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

pub const LOCAL: &str = "local";

pub enum DockerBackend {
    Api { docker: Docker, _bridge: Option<Bridge> },
    Cli { conn: Arc<SshConn> },
}

/// Local unix socket bridged to a remote socket; removed on drop.
pub struct Bridge {
    dir: PathBuf,
    task: tokio::task::AbortHandle,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub created: i64,
    pub ports: Vec<String>,
    pub project: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub id: String,
    pub tags: Vec<String>,
    pub size: i64,
    pub created: i64,
    pub containers: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EndpointInfo {
    pub endpoint: String,
    pub mode: String,
    pub server_version: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove,
}

fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir())
}

async fn bridge(conn: Arc<SshConn>, remote_socket: String) -> Result<(Docker, Bridge)> {
    let dir = runtime_dir().join(format!("shellport-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let sock = dir.join("docker.sock");
    let listener = tokio::net::UnixListener::bind(&sock)?;
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { break };
            let conn = conn.clone();
            let remote = remote_socket.clone();
            tokio::spawn(async move {
                match conn.handle.channel_open_direct_streamlocal(remote).await {
                    Ok(ch) => {
                        let mut cs = ch.into_stream();
                        let _ = tokio::io::copy_bidirectional(&mut s, &mut cs).await;
                    }
                    Err(e) => log::warn!("docker streamlocal open failed: {e}"),
                }
            });
        }
    })
    .abort_handle();
    let b = Bridge { dir, task };
    let docker = Docker::connect_with_unix(sock.to_str().unwrap(), 120, API_DEFAULT_VERSION)?;
    Ok((docker, b))
}

/// Resolve (and cache) the Docker backend for an endpoint id: "local" or a host id.
pub async fn backend(app: &AppHandle, endpoint: &str) -> Result<Arc<DockerBackend>> {
    let state = app.state::<AppState>();
    if let Some(b) = state.dockers.lock().await.get(endpoint) {
        let alive = match &**b {
            DockerBackend::Cli { conn } => !conn.is_closed(),
            DockerBackend::Api { .. } => true,
        };
        if alive {
            return Ok(b.clone());
        }
    }
    let b = if endpoint == LOCAL {
        let docker = Docker::connect_with_local_defaults()?;
        docker.ping().await.context(
            "cannot reach the local Docker daemon (is it running and is your user in the `docker` group?)",
        )?;
        DockerBackend::Api { docker, _bridge: None }
    } else {
        let conn = crate::ssh::get_conn(app.clone(), endpoint.to_string(), 0).await?;
        let socket = conn.host.docker_socket.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "/var/run/docker.sock".into());
        let api = async {
            let (docker, bridge) = bridge(conn.clone(), socket.clone()).await?;
            tokio::time::timeout(std::time::Duration::from_secs(8), docker.ping()).await??;
            anyhow::Ok(DockerBackend::Api { docker, _bridge: Some(bridge) })
        }
        .await;
        match api {
            Ok(b) => b,
            Err(e) => {
                log::info!("docker socket tunnel unavailable ({e:#}); falling back to docker CLI over SSH");
                conn.exec_capture("docker version --format '{{.Server.Version}}'")
                    .await
                    .with_context(|| format!("Docker not reachable on {} via socket ({e:#}) or CLI", conn.host.name))?;
                DockerBackend::Cli { conn }
            }
        }
    };
    let b = Arc::new(b);
    state.dockers.lock().await.insert(endpoint.to_string(), b.clone());
    Ok(b)
}

pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(crate) fn valid_ref(id: &str) -> Result<()> {
    if !id.starts_with(|c: char| c.is_ascii_alphanumeric()) || !id.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)) {
        bail!("invalid container reference");
    }
    Ok(())
}

impl DockerBackend {
    pub async fn info(&self, endpoint: &str) -> Result<EndpointInfo> {
        Ok(match self {
            DockerBackend::Api { docker, _bridge } => EndpointInfo {
                endpoint: endpoint.into(),
                mode: if _bridge.is_some() { "ssh-socket".into() } else { "local".into() },
                server_version: docker.version().await.ok().and_then(|v| v.version),
            },
            DockerBackend::Cli { conn } => EndpointInfo {
                endpoint: endpoint.into(),
                mode: "ssh-cli".into(),
                server_version: conn
                    .exec_capture("docker version --format '{{.Server.Version}}'")
                    .await
                    .ok()
                    .map(|s| s.trim().to_string()),
            },
        })
    }

    pub async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        match self {
            DockerBackend::Api { docker, .. } => {
                let opts = ListContainersOptionsBuilder::new().all(all).build();
                let list = docker.list_containers(Some(opts)).await?;
                Ok(list
                    .into_iter()
                    .map(|c| {
                        let mut ports: Vec<String> = c
                            .ports
                            .unwrap_or_default()
                            .into_iter()
                            .map(|p| match p.public_port {
                                Some(pp) => format!("{}:{}->{}", p.ip.unwrap_or_default(), pp, p.private_port),
                                None => format!("{}", p.private_port),
                            })
                            .collect();
                        ports.dedup();
                        ContainerInfo {
                            id: c.id.unwrap_or_default(),
                            name: c
                                .names
                                .and_then(|n| n.into_iter().next())
                                .map(|n| n.trim_start_matches('/').to_string())
                                .unwrap_or_default(),
                            image: c.image.unwrap_or_default(),
                            state: c.state.map(|s| s.to_string()).unwrap_or_default(),
                            status: c.status.unwrap_or_default(),
                            created: c.created.unwrap_or(0),
                            ports,
                            project: c.labels.and_then(|l| l.get("com.docker.compose.project").cloned()),
                        }
                    })
                    .collect())
            }
            DockerBackend::Cli { conn } => {
                let flag = if all { "-a " } else { "" };
                let out = conn
                    .exec_capture(&format!("docker ps {flag}--no-trunc --format '{{{{json .}}}}'"))
                    .await?;
                let mut v = Vec::new();
                for line in out.lines().filter(|l| !l.trim().is_empty()) {
                    let j: serde_json::Value = serde_json::from_str(line)?;
                    let s = |k: &str| j.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let labels = s("Labels");
                    v.push(ContainerInfo {
                        id: s("ID"),
                        name: s("Names"),
                        image: s("Image"),
                        state: s("State"),
                        status: s("Status"),
                        created: 0,
                        ports: s("Ports").split(", ").filter(|p| !p.is_empty()).map(String::from).collect(),
                        project: labels
                            .split(',')
                            .find_map(|kv| kv.strip_prefix("com.docker.compose.project="))
                            .map(String::from),
                    });
                }
                Ok(v)
            }
        }
    }

    pub async fn images(&self) -> Result<Vec<ImageInfo>> {
        match self {
            DockerBackend::Api { docker, .. } => {
                let list = docker.list_images(Some(ListImagesOptionsBuilder::new().all(false).build())).await?;
                Ok(list
                    .into_iter()
                    .map(|i| ImageInfo { id: i.id, tags: i.repo_tags, size: i.size, created: i.created, containers: i.containers })
                    .collect())
            }
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture("docker images --no-trunc --format '{{json .}}'").await?;
                let mut v = Vec::new();
                for line in out.lines().filter(|l| !l.trim().is_empty()) {
                    let j: serde_json::Value = serde_json::from_str(line)?;
                    let s = |k: &str| j.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    v.push(ImageInfo {
                        id: s("ID"),
                        tags: vec![format!("{}:{}", s("Repository"), s("Tag"))],
                        size: 0,
                        created: 0,
                        containers: -1,
                    });
                }
                Ok(v)
            }
        }
    }

    pub async fn action(&self, id: &str, action: Action) -> Result<()> {
        valid_ref(id)?;
        match self {
            DockerBackend::Api { docker, .. } => {
                match action {
                    Action::Start => docker.start_container(id, None).await?,
                    Action::Stop => docker.stop_container(id, Some(StopContainerOptionsBuilder::new().t(10).build())).await?,
                    Action::Restart => docker.restart_container(id, Some(RestartContainerOptionsBuilder::new().t(10).build())).await?,
                    Action::Pause => docker.pause_container(id).await?,
                    Action::Unpause => docker.unpause_container(id).await?,
                    Action::Kill => docker.kill_container(id, None).await?,
                    Action::Remove => docker.remove_container(id, Some(RemoveContainerOptionsBuilder::new().force(true).build())).await?,
                }
                Ok(())
            }
            DockerBackend::Cli { conn } => {
                let verb = match action {
                    Action::Start => "start",
                    Action::Stop => "stop",
                    Action::Restart => "restart",
                    Action::Pause => "pause",
                    Action::Unpause => "unpause",
                    Action::Kill => "kill",
                    Action::Remove => "rm -f",
                };
                conn.exec_capture(&format!("docker {verb} {}", shell_quote(id))).await?;
                Ok(())
            }
        }
    }

    pub async fn inspect(&self, id: &str) -> Result<serde_json::Value> {
        valid_ref(id)?;
        match self {
            DockerBackend::Api { docker, .. } => Ok(serde_json::to_value(docker.inspect_container(id, None).await?)?),
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture(&format!("docker inspect {}", shell_quote(id))).await?;
                let v: serde_json::Value = serde_json::from_str(&out)?;
                Ok(v.get(0).cloned().unwrap_or(v))
            }
        }
    }
}

/// Stream container logs into `out`. Returns a task id usable to stop it.
pub async fn stream_logs(
    app: AppHandle,
    endpoint: String,
    id: String,
    tail: u32,
    timestamps: bool,
    follow: bool,
    out: Channel<TermEvent>,
) -> Result<String> {
    valid_ref(&id)?;
    let b = backend(&app, &endpoint).await?;
    let task_id = format!("logs:{}", uuid::Uuid::new_v4());
    let h = match &*b {
        DockerBackend::Api { docker, .. } => {
            let opts = LogsOptionsBuilder::new()
                .follow(follow)
                .stdout(true)
                .stderr(true)
                .timestamps(timestamps)
                .tail(&tail.to_string())
                .build();
            let mut stream = docker.logs(&id, Some(opts));
            let b = b.clone();
            tokio::spawn(async move {
                let _keep = b;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(o) => {
                            if out.send(data_event(&o.into_bytes())).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = out.send(TermEvent::Error { message: e.to_string() });
                            break;
                        }
                    }
                }
                let _ = out.send(TermEvent::Exit { code: None });
            })
        }
        DockerBackend::Cli { conn } => {
            let mut ch = conn.handle.channel_open_session().await?;
            let cmd = format!(
                "docker logs {}{}--tail {} {}",
                if follow { "-f " } else { "" },
                if timestamps { "-t " } else { "" },
                tail,
                shell_quote(&id)
            );
            ch.exec(true, cmd).await?;
            let conn = conn.clone();
            tokio::spawn(async move {
                let _keep = conn;
                while let Some(msg) = ch.wait().await {
                    match msg {
                        russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                            if out.send(data_event(&data)).is_err() {
                                let _ = ch.close().await;
                                return;
                            }
                        }
                        russh::ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                let _ = out.send(TermEvent::Exit { code: None });
            })
        }
    };
    app.state::<AppState>().add_task(task_id.clone(), h.abort_handle());
    Ok(task_id)
}

/// Interactive shell inside a container.
pub async fn exec_shell(
    app: AppHandle,
    endpoint: String,
    id: String,
    shell: Option<String>,
    user: Option<String>,
    cols: u16,
    rows: u16,
    out: Channel<TermEvent>,
) -> Result<String> {
    valid_ref(&id)?;
    let b = backend(&app, &endpoint).await?;
    match &*b {
        DockerBackend::Api { docker, .. } => {
            let cmd = match shell.filter(|s| !s.trim().is_empty()) {
                Some(s) => vec![s],
                None => vec!["/bin/sh".into(), "-c".into(), "command -v bash >/dev/null && exec bash || exec sh".into()],
            };
            crate::term::open_docker_exec(app.clone(), docker.clone(), id, cmd, user, cols, rows, out).await
        }
        DockerBackend::Cli { conn } => {
            let user_flag = user.filter(|u| !u.is_empty()).map(|u| format!("-u {} ", shell_quote(&u))).unwrap_or_default();
            let inner = match shell.filter(|s| !s.trim().is_empty()) {
                Some(s) => shell_quote(&s),
                None => "sh -c 'command -v bash >/dev/null && exec bash || exec sh'".into(),
            };
            let cmd = format!("docker exec -it {user_flag}{} {inner}", shell_quote(&id));
            crate::term::open_ssh(app.clone(), conn.host.id.clone(), cols, rows, Some(cmd), out).await
        }
    }
}

pub fn ensure_endpoint(endpoint: &str) -> Result<()> {
    if endpoint.is_empty() {
        return Err(anyhow!("no Docker endpoint selected"));
    }
    Ok(())
}
