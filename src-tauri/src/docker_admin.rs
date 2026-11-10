//! Docker administration beyond single containers: disk usage, prune, volumes,
//! networks, image pull/remove, compose-project actions and combined logs.
//! Every operation works on both backends (Docker API or `docker` CLI over SSH).

use crate::docker::{backend, shell_quote, Action, DockerBackend};
use crate::state::AppState;
use crate::term::{data_event, TermEvent};
use anyhow::{bail, Result};
use bollard::query_parameters::{
    CreateImageOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder, PruneBuildOptionsBuilder,
    PruneImagesOptionsBuilder, PruneVolumesOptionsBuilder, RemoveImageOptionsBuilder, RemoveVolumeOptionsBuilder,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

// ---------- DTOs ----------

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    pub kind: String,
    pub total: i64,
    pub active: i64,
    pub size: Option<i64>,
    pub reclaimable: Option<i64>,
    /// Human-readable sizes from the CLI when byte counts are unavailable.
    pub size_text: Option<String>,
    pub reclaimable_text: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PruneTarget {
    Containers,
    Images,
    Volumes,
    Networks,
    BuildCache,
    System,
}

#[derive(Deserialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PruneOptions {
    /// Images: all unused (not only dangling). Volumes: named too (not only anonymous).
    /// Build cache: everything. System: same as `docker system prune -a`.
    pub all: bool,
    /// System prune only: also remove unused volumes.
    pub volumes: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PruneResult {
    pub removed: usize,
    pub reclaimed: Option<i64>,
    pub reclaimed_text: Option<String>,
    pub details: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: Option<String>,
    pub project: Option<String>,
    pub anonymous: bool,
    pub used_by: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub internal: bool,
    pub project: Option<String>,
    pub builtin: bool,
    pub containers: Vec<String>,
}

// ---------- helpers ----------

/// First `n` characters (never splits a multi-byte character).
fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn valid_name(s: &str, extra: &str) -> Result<()> {
    // Must start alphanumeric: a leading '-' would be parsed as a CLI flag.
    if s.is_empty() || s.len() > 255 || !s.starts_with(|c: char| c.is_ascii_alphanumeric()) || !s.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c) || extra.contains(c)) {
        bail!("invalid name: {s:?}");
    }
    Ok(())
}

const PROJECT_LABEL: &str = "com.docker.compose.project";

fn label_from_cli(labels: &str, key: &str) -> Option<String> {
    labels.split(',').find_map(|kv| kv.strip_prefix(&format!("{key}="))).map(String::from)
}

fn json_lines(out: &str) -> Result<Vec<serde_json::Value>> {
    out.lines().filter(|l| !l.trim().is_empty()).map(|l| Ok(serde_json::from_str(l)?)).collect()
}

fn s(j: &serde_json::Value, k: &str) -> String {
    j.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

/// "Total reclaimed space: 1.2GB" → Some("1.2GB")
fn reclaimed_from_cli(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("Total reclaimed space:").map(|v| v.trim().to_string()))
}

/// Rough count of "deleted:"/"untagged:"/id lines in CLI prune output.
fn cli_removed_lines(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Total reclaimed") && !l.ends_with(':') && !l.starts_with("WARNING"))
        .map(String::from)
        .collect()
}

/// Container name → (volume names, network names), from a full container listing.
struct Usage {
    volumes: HashMap<String, Vec<String>>,
    networks: HashMap<String, Vec<String>>,
}

impl DockerBackend {
    async fn usage_map(&self) -> Result<Usage> {
        let mut volumes: HashMap<String, Vec<String>> = HashMap::new();
        let mut networks: HashMap<String, Vec<String>> = HashMap::new();
        match self {
            DockerBackend::Api { docker, .. } => {
                let list = docker.list_containers(Some(ListContainersOptionsBuilder::new().all(true).build())).await?;
                for c in list {
                    let name = c
                        .names
                        .and_then(|n| n.into_iter().next())
                        .map(|n| n.trim_start_matches('/').to_string())
                        .unwrap_or_default();
                    for m in c.mounts.unwrap_or_default() {
                        if let Some(v) = m.name {
                            volumes.entry(v).or_default().push(name.clone());
                        }
                    }
                    if let Some(ns) = c.network_settings.and_then(|n| n.networks) {
                        for net in ns.keys() {
                            networks.entry(net.clone()).or_default().push(name.clone());
                        }
                    }
                }
            }
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture("docker ps -a --no-trunc --format '{{json .}}'").await?;
                for j in json_lines(&out)? {
                    let name = s(&j, "Names");
                    for v in s(&j, "Mounts").split(',').map(str::trim).filter(|v| !v.is_empty()) {
                        volumes.entry(v.to_string()).or_default().push(name.clone());
                    }
                    for n in s(&j, "Networks").split(',').map(str::trim).filter(|v| !v.is_empty()) {
                        networks.entry(n.to_string()).or_default().push(name.clone());
                    }
                }
            }
        }
        Ok(Usage { volumes, networks })
    }

    // ---------- disk usage ----------

    pub async fn disk_usage(&self) -> Result<Vec<UsageRow>> {
        match self {
            DockerBackend::Api { docker, .. } => {
                let df = docker.df(None).await?;
                let row = |kind: &str, total: Option<i64>, active: Option<i64>, size: Option<i64>, rec: Option<i64>| UsageRow {
                    kind: kind.into(),
                    total: total.unwrap_or(0),
                    active: active.unwrap_or(0),
                    size,
                    reclaimable: rec,
                    ..Default::default()
                };
                let mut v = Vec::new();
                if let Some(u) = df.image_usage {
                    v.push(row("Images", u.total_count, u.active_count, u.total_size, u.reclaimable));
                }
                if let Some(u) = df.container_usage {
                    v.push(row("Containers", u.total_count, u.active_count, u.total_size, u.reclaimable));
                }
                if let Some(u) = df.volume_usage {
                    v.push(row("Local Volumes", u.total_count, u.active_count, u.total_size, u.reclaimable));
                }
                if let Some(u) = df.build_cache_usage {
                    v.push(row("Build Cache", u.total_count, u.active_count, u.total_size, u.reclaimable));
                }
                if v.is_empty() {
                    // Engines older than API 1.52 don't send the *Usage summaries: count from listings.
                    let images = docker.list_images(None::<bollard::query_parameters::ListImagesOptions>).await?;
                    let containers = self.list(true).await?;
                    let vols = self.volumes().await?;
                    let size: i64 = images.iter().map(|i| i.size).sum();
                    v.push(row("Images", Some(images.len() as i64), Some(images.iter().filter(|i| i.containers > 0).count() as i64), Some(size), None));
                    v.push(row("Containers", Some(containers.len() as i64), Some(containers.iter().filter(|c| c.state == "running").count() as i64), None, None));
                    v.push(row("Local Volumes", Some(vols.len() as i64), Some(vols.iter().filter(|x| !x.used_by.is_empty()).count() as i64), None, None));
                }
                Ok(v)
            }
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture("docker system df --format '{{json .}}'").await?;
                Ok(json_lines(&out)?
                    .into_iter()
                    .map(|j| UsageRow {
                        kind: s(&j, "Type"),
                        total: s(&j, "TotalCount").parse().unwrap_or(0),
                        active: s(&j, "Active").parse().unwrap_or(0),
                        size: None,
                        reclaimable: None,
                        size_text: Some(s(&j, "Size")),
                        reclaimable_text: Some(s(&j, "Reclaimable")),
                    })
                    .collect())
            }
        }
    }

    // ---------- prune ----------

    pub async fn prune(&self, target: PruneTarget, opts: PruneOptions) -> Result<PruneResult> {
        match self {
            DockerBackend::Api { docker, .. } => {
                let mut r = PruneResult::default();
                let mut add = |n: usize, space: Option<i64>, details: Vec<String>| {
                    r.removed += n;
                    if let Some(sp) = space {
                        r.reclaimed = Some(r.reclaimed.unwrap_or(0) + sp);
                    }
                    r.details.extend(details);
                };
                let do_containers = matches!(target, PruneTarget::Containers | PruneTarget::System);
                let do_networks = matches!(target, PruneTarget::Networks | PruneTarget::System);
                let do_images = matches!(target, PruneTarget::Images | PruneTarget::System);
                let do_build = matches!(target, PruneTarget::BuildCache | PruneTarget::System);
                let do_volumes = target == PruneTarget::Volumes || (target == PruneTarget::System && opts.volumes);

                if do_containers {
                    let x = docker.prune_containers(None).await?;
                    let d = x.containers_deleted.unwrap_or_default();
                    add(d.len(), x.space_reclaimed, d.into_iter().map(|i| format!("container {}", short(&i, 12))).collect());
                }
                if do_networks {
                    let x = docker.prune_networks(None).await?;
                    let d = x.networks_deleted.unwrap_or_default();
                    add(d.len(), None, d.into_iter().map(|n| format!("network {n}")).collect());
                }
                if do_volumes {
                    let mut f: HashMap<&str, Vec<&str>> = HashMap::new();
                    if opts.all {
                        f.insert("all", vec!["true"]);
                    }
                    let x = docker.prune_volumes(Some(PruneVolumesOptionsBuilder::new().filters(&f).build())).await?;
                    let d = x.volumes_deleted.unwrap_or_default();
                    add(d.len(), x.space_reclaimed, d.into_iter().map(|v| format!("volume {v}")).collect());
                }
                if do_images {
                    let mut f: HashMap<&str, Vec<&str>> = HashMap::new();
                    f.insert("dangling", vec![if opts.all { "false" } else { "true" }]);
                    let x = docker.prune_images(Some(PruneImagesOptionsBuilder::new().filters(&f).build())).await?;
                    let d = x.images_deleted.unwrap_or_default();
                    let lines: Vec<String> = d
                        .iter()
                        .filter_map(|i| i.untagged.as_ref().map(|u| format!("untagged {u}")).or_else(|| i.deleted.as_ref().map(|u| format!("deleted {}", short(u, 19)))))
                        .collect();
                    let n = d.iter().filter(|i| i.deleted.is_some()).count();
                    add(n, x.space_reclaimed, lines);
                }
                if do_build {
                    match docker.prune_build(Some(PruneBuildOptionsBuilder::new().all(opts.all).build())).await {
                        Ok(x) => {
                            let d = x.caches_deleted.unwrap_or_default();
                            add(d.len(), x.space_reclaimed, vec![]);
                        }
                        // Build cache prune needs BuildKit; don't fail a system prune because of it.
                        Err(e) if target == PruneTarget::System => r.details.push(format!("build cache skipped: {e}")),
                        Err(e) => return Err(e.into()),
                    }
                }
                Ok(r)
            }
            DockerBackend::Cli { conn } => {
                let a = if opts.all { " -a" } else { "" };
                let cmd = match target {
                    PruneTarget::Containers => "docker container prune -f".to_string(),
                    PruneTarget::Images => format!("docker image prune -f{a}"),
                    PruneTarget::Volumes => format!("docker volume prune -f{a}"),
                    PruneTarget::Networks => "docker network prune -f".to_string(),
                    PruneTarget::BuildCache => format!("docker builder prune -f{a}"),
                    PruneTarget::System => format!("docker system prune -f{a}{}", if opts.volumes { " --volumes" } else { "" }),
                };
                let out = conn.exec_capture(&cmd).await?;
                let details = cli_removed_lines(&out);
                Ok(PruneResult { removed: details.len(), reclaimed: None, reclaimed_text: reclaimed_from_cli(&out), details })
            }
        }
    }

    // ---------- volumes ----------

    pub async fn volumes(&self) -> Result<Vec<VolumeInfo>> {
        let usage = self.usage_map().await?;
        let mut v = match self {
            DockerBackend::Api { docker, .. } => {
                let res = docker.list_volumes(None::<bollard::query_parameters::ListVolumesOptions>).await?;
                res.volumes
                    .unwrap_or_default()
                    .into_iter()
                    .map(|vol| VolumeInfo {
                        anonymous: vol.labels.contains_key("com.docker.volume.anonymous")
                            || (vol.name.len() == 64 && vol.name.chars().all(|c| c.is_ascii_hexdigit())),
                        project: vol.labels.get(PROJECT_LABEL).cloned(),
                        created: vol.created_at.map(|d| d.to_string()),
                        used_by: usage.volumes.get(&vol.name).cloned().unwrap_or_default(),
                        name: vol.name,
                        driver: vol.driver,
                        mountpoint: vol.mountpoint,
                    })
                    .collect::<Vec<_>>()
            }
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture("docker volume ls --format '{{json .}}'").await?;
                json_lines(&out)?
                    .into_iter()
                    .map(|j| {
                        let name = s(&j, "Name");
                        let labels = s(&j, "Labels");
                        VolumeInfo {
                            anonymous: labels.contains("com.docker.volume.anonymous") || (name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit())),
                            project: label_from_cli(&labels, PROJECT_LABEL),
                            created: None,
                            used_by: usage.volumes.get(&name).cloned().unwrap_or_default(),
                            driver: s(&j, "Driver"),
                            mountpoint: s(&j, "Mountpoint"),
                            name,
                        }
                    })
                    .collect()
            }
        };
        v.sort_by(|a, b| a.anonymous.cmp(&b.anonymous).then_with(|| a.name.cmp(&b.name)));
        Ok(v)
    }

    pub async fn volume_create(&self, name: &str) -> Result<()> {
        valid_name(name, "")?;
        match self {
            DockerBackend::Api { docker, .. } => {
                docker
                    .create_volume(bollard::models::VolumeCreateRequest { name: Some(name.to_string()), ..Default::default() })
                    .await?;
            }
            DockerBackend::Cli { conn } => {
                conn.exec_capture(&format!("docker volume create {}", shell_quote(name))).await?;
            }
        }
        Ok(())
    }

    pub async fn volume_remove(&self, name: &str, force: bool) -> Result<()> {
        valid_name(name, "")?;
        match self {
            DockerBackend::Api { docker, .. } => docker.remove_volume(name, Some(RemoveVolumeOptionsBuilder::new().force(force).build())).await?,
            DockerBackend::Cli { conn } => {
                conn.exec_capture(&format!("docker volume rm {}{}", if force { "-f " } else { "" }, shell_quote(name))).await?;
            }
        }
        Ok(())
    }

    pub async fn volume_inspect(&self, name: &str) -> Result<serde_json::Value> {
        valid_name(name, "")?;
        match self {
            DockerBackend::Api { docker, .. } => Ok(serde_json::to_value(docker.inspect_volume(name).await?)?),
            DockerBackend::Cli { conn } => {
                let v: serde_json::Value = serde_json::from_str(&conn.exec_capture(&format!("docker volume inspect {}", shell_quote(name))).await?)?;
                Ok(v.get(0).cloned().unwrap_or(v))
            }
        }
    }

    // ---------- networks ----------

    pub async fn networks(&self) -> Result<Vec<NetworkInfo>> {
        let usage = self.usage_map().await?;
        let builtin = |n: &str| matches!(n, "bridge" | "host" | "none");
        let mut v = match self {
            DockerBackend::Api { docker, .. } => docker
                .list_networks(None)
                .await?
                .into_iter()
                .map(|n| {
                    let name = n.name.unwrap_or_default();
                    let cfg = n.ipam.and_then(|i| i.config).and_then(|c| c.into_iter().next());
                    NetworkInfo {
                        id: n.id.unwrap_or_default(),
                        driver: n.driver.unwrap_or_default(),
                        scope: n.scope.unwrap_or_default(),
                        subnet: cfg.as_ref().and_then(|c| c.subnet.clone()),
                        gateway: cfg.as_ref().and_then(|c| c.gateway.clone()),
                        internal: n.internal.unwrap_or(false),
                        project: n.labels.as_ref().and_then(|l| l.get(PROJECT_LABEL).cloned()),
                        builtin: builtin(&name),
                        containers: usage.networks.get(&name).cloned().unwrap_or_default(),
                        name,
                    }
                })
                .collect::<Vec<_>>(),
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture("docker network ls --no-trunc --format '{{json .}}'").await?;
                json_lines(&out)?
                    .into_iter()
                    .map(|j| {
                        let name = s(&j, "Name");
                        NetworkInfo {
                            id: s(&j, "ID"),
                            driver: s(&j, "Driver"),
                            scope: s(&j, "Scope"),
                            subnet: None,
                            gateway: None,
                            internal: s(&j, "Internal") == "true",
                            project: label_from_cli(&s(&j, "Labels"), PROJECT_LABEL),
                            builtin: builtin(&name),
                            containers: usage.networks.get(&name).cloned().unwrap_or_default(),
                            name,
                        }
                    })
                    .collect()
            }
        };
        v.sort_by(|a, b| a.builtin.cmp(&b.builtin).then_with(|| a.name.cmp(&b.name)));
        Ok(v)
    }

    pub async fn network_create(&self, name: &str, internal: bool) -> Result<()> {
        valid_name(name, "")?;
        match self {
            DockerBackend::Api { docker, .. } => {
                docker
                    .create_network(bollard::models::NetworkCreateRequest {
                        name: name.to_string(),
                        driver: Some("bridge".into()),
                        internal: Some(internal),
                        ..Default::default()
                    })
                    .await?;
            }
            DockerBackend::Cli { conn } => {
                conn.exec_capture(&format!("docker network create {}{}", if internal { "--internal " } else { "" }, shell_quote(name))).await?;
            }
        }
        Ok(())
    }

    pub async fn network_remove(&self, id: &str) -> Result<()> {
        valid_name(id, "")?;
        match self {
            DockerBackend::Api { docker, .. } => docker.remove_network(id).await?,
            DockerBackend::Cli { conn } => {
                conn.exec_capture(&format!("docker network rm {}", shell_quote(id))).await?;
            }
        }
        Ok(())
    }

    pub async fn network_inspect(&self, id: &str) -> Result<serde_json::Value> {
        valid_name(id, "")?;
        match self {
            DockerBackend::Api { docker, .. } => Ok(serde_json::to_value(docker.inspect_network(id, None).await?)?),
            DockerBackend::Cli { conn } => {
                let v: serde_json::Value = serde_json::from_str(&conn.exec_capture(&format!("docker network inspect {}", shell_quote(id))).await?)?;
                Ok(v.get(0).cloned().unwrap_or(v))
            }
        }
    }

    // ---------- images ----------

    pub async fn image_remove(&self, reference: &str, force: bool) -> Result<Vec<String>> {
        valid_name(reference, ":/@")?;
        match self {
            DockerBackend::Api { docker, .. } => {
                let r = docker.remove_image(reference, Some(RemoveImageOptionsBuilder::new().force(force).build()), None).await?;
                Ok(r.into_iter()
                    .filter_map(|i| i.untagged.map(|u| format!("untagged {u}")).or(i.deleted.map(|d| format!("deleted {d}"))))
                    .collect())
            }
            DockerBackend::Cli { conn } => {
                let out = conn.exec_capture(&format!("docker rmi {}{}", if force { "-f " } else { "" }, shell_quote(reference))).await?;
                Ok(out.lines().map(String::from).collect())
            }
        }
    }
}

/// Pull an image, streaming progress lines into `out`.
/// Registry host of an image reference ("docker.io" when none is given).
pub fn registry_of(reference: &str) -> String {
    match reference.split_once('/') {
        Some((first, _)) if first.contains('.') || first.contains(':') || first == "localhost" => first.to_string(),
        _ => "docker.io".to_string(),
    }
}

/// Registry credentials taken from a vault secret (username + password/token).
pub struct RegistryAuth {
    pub username: String,
    pub password: zeroize::Zeroizing<String>,
}

pub fn registry_auth(app: &AppHandle, secret_id: &str) -> Result<RegistryAuth> {
    let state = app.state::<AppState>();
    let vault = state.vault.lock();
    if !vault.is_unlocked() {
        bail!("unlock the vault to use registry credentials");
    }
    let s = vault.get(secret_id)?;
    let username = s.username.clone().filter(|u| !u.is_empty()).ok_or_else(|| anyhow::anyhow!("secret '{}' has no username set", s.name))?;
    Ok(RegistryAuth { username, password: zeroize::Zeroizing::new(s.value.clone()) })
}

pub async fn image_pull(app: AppHandle, endpoint: String, reference: String, secret_id: Option<String>, out: Channel<TermEvent>) -> Result<String> {
    valid_name(&reference, ":/@")?;
    let auth = secret_id.as_deref().filter(|s| !s.is_empty()).map(|id| registry_auth(&app, id)).transpose()?;
    let registry = registry_of(&reference);
    let b = backend(&app, &endpoint).await?;
    let task_id = format!("pull:{}", uuid::Uuid::new_v4());
    let h = match &*b {
        DockerBackend::Api { docker, .. } => {
            // Digest references are pulled as-is; otherwise split off the tag.
            let (image, tag) = if reference.contains('@') {
                (reference.clone(), String::new())
            } else {
                match reference.rsplit_once(':').filter(|(_, t)| !t.contains('/')) {
                    Some((i, t)) => (i.to_string(), t.to_string()),
                    None => (reference.clone(), "latest".to_string()),
                }
            };
            let creds = auth.as_ref().map(|a| bollard::auth::DockerCredentials {
                username: Some(a.username.clone()),
                password: Some(a.password.to_string()),
                serveraddress: Some(if registry == "docker.io" { "https://index.docker.io/v1/".into() } else { registry.clone() }),
                ..Default::default()
            });
            let docker = docker.clone();
            let b = b.clone();
            tokio::spawn(async move {
                let _keep = b;
                let mut builder = CreateImageOptionsBuilder::new().from_image(&image);
                if !tag.is_empty() {
                    builder = builder.tag(&tag);
                }
                let mut stream = docker.create_image(Some(builder.build()), None, creds);
                let mut last: HashMap<String, String> = HashMap::new();
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(info) => {
                            if let Some(err) = info.error_detail.and_then(|e| e.message) {
                                let _ = out.send(TermEvent::Error { message: err });
                                break;
                            }
                            let status = info.status.unwrap_or_default();
                            let id = info.id.unwrap_or_default();
                            // Only emit status changes per layer, not every byte of progress.
                            if last.get(&id) != Some(&status) {
                                last.insert(id.clone(), status.clone());
                                let line = if id.is_empty() { format!("{status}\r\n") } else { format!("{id}: {status}\r\n") };
                                let _ = out.send(data_event(line.as_bytes()));
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
            match &auth {
                // Log in to a throwaway DOCKER_CONFIG so nothing is persisted on the server;
                // the password goes over stdin, never on the command line.
                Some(a) => {
                    let script = format!(
                        "D=$(mktemp -d) && trap 'rm -rf \"$D\"' EXIT && DOCKER_CONFIG=\"$D\" docker login -u {} --password-stdin {} >/dev/null && DOCKER_CONFIG=\"$D\" docker pull {}",
                        shell_quote(&a.username),
                        shell_quote(&registry),
                        shell_quote(&reference)
                    );
                    ch.exec(true, format!("sh -c {} 2>&1", shell_quote(&script))).await?;
                    ch.data(a.password.as_bytes()).await?;
                    ch.eof().await?;
                }
                None => ch.exec(true, format!("docker pull {} 2>&1", shell_quote(&reference))).await?,
            }
            let conn = conn.clone();
            tokio::spawn(async move {
                let _keep = conn;
                let mut code = None;
                while let Some(msg) = ch.wait().await {
                    match msg {
                        russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                            let text = String::from_utf8_lossy(&data).replace('\n', "\r\n");
                            let _ = out.send(data_event(text.as_bytes()));
                        }
                        russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                        russh::ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                let _ = out.send(TermEvent::Exit { code });
            })
        }
    };
    app.state::<AppState>().add_task(task_id.clone(), h.abort_handle());
    Ok(task_id)
}

// ---------- compose projects ----------

/// Container ids and names belonging to a compose project.
pub async fn project_containers(b: &DockerBackend, project: &str) -> Result<Vec<(String, String, String)>> {
    valid_name(project, "")?;
    Ok(b.list(true)
        .await?
        .into_iter()
        .filter(|c| c.project.as_deref() == Some(project))
        .map(|c| (c.id, c.name, c.state))
        .collect())
}

/// Start / stop / restart every container of a compose project.
pub async fn project_action(b: &DockerBackend, project: &str, action: Action) -> Result<usize> {
    if !matches!(action, Action::Start | Action::Stop | Action::Restart) {
        bail!("only start, stop and restart are supported for projects");
    }
    let list = project_containers(b, project).await?;
    if list.is_empty() {
        bail!("no containers found for project {project}");
    }
    let targets: Vec<_> = list
        .into_iter()
        .filter(|(_, _, state)| match action {
            Action::Start => state != "running",
            Action::Stop => state == "running" || state == "paused" || state == "restarting",
            _ => true,
        })
        .collect();
    let futs = targets.iter().map(|(id, _, _)| b.action(id, action));
    let results = futures_util::future::join_all(futs).await;
    let failed: Vec<String> = results
        .into_iter()
        .zip(&targets)
        .filter_map(|(r, (_, name, _))| r.err().map(|e| format!("{name}: {e:#}")))
        .collect();
    if !failed.is_empty() {
        bail!("{}", failed.join("; "));
    }
    Ok(targets.len())
}

// ---------- combined logs ----------

const COLORS: [&str; 8] = ["36", "33", "35", "32", "34", "91", "96", "93"];

/// Follow logs of several containers at once, interleaved line by line with a
/// coloured `name |` prefix (like `docker compose logs`).
pub async fn stream_multi_logs(
    app: AppHandle,
    endpoint: String,
    containers: Vec<(String, String)>,
    tail: u32,
    timestamps: bool,
    follow: bool,
    out: Channel<TermEvent>,
) -> Result<String> {
    if containers.is_empty() {
        bail!("no containers selected");
    }
    for (id, _) in &containers {
        crate::docker::valid_ref(id)?;
    }
    let b = backend(&app, &endpoint).await?;
    let task_id = format!("mlogs:{}", uuid::Uuid::new_v4());
    let width = containers.iter().map(|(_, n)| n.len()).max().unwrap_or(0).min(28);
    let (tx, mut rx) = mpsc::channel::<(usize, Option<Vec<u8>>)>(256);

    let mut producers = Vec::new();
    for (idx, (id, _)) in containers.iter().enumerate() {
        let tx = tx.clone();
        let id = id.clone();
        let h = match &*b {
            DockerBackend::Api { docker, .. } => {
                let opts = LogsOptionsBuilder::new()
                    .follow(follow)
                    .stdout(true)
                    .stderr(true)
                    // Always timestamped: used to order the backlog, then stripped
                    // again unless the user asked to see them.
                    .timestamps(true)
                    .tail(&tail.to_string())
                    .build();
                let mut stream = docker.logs(&id, Some(opts));
                tokio::spawn(async move {
                    while let Some(Ok(o)) = stream.next().await {
                        if tx.send((idx, Some(o.into_bytes().to_vec()))).await.is_err() {
                            return;
                        }
                    }
                    let _ = tx.send((idx, None)).await;
                })
            }
            DockerBackend::Cli { conn } => {
                let mut ch = conn.handle.channel_open_session().await?;
                let cmd = format!(
                    "docker logs {}-t --tail {} {}",
                    if follow { "-f " } else { "" },
                    tail,
                    shell_quote(&id)
                );
                ch.exec(true, cmd).await?;
                tokio::spawn(async move {
                    while let Some(msg) = ch.wait().await {
                        match msg {
                            russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                                if tx.send((idx, Some(data.to_vec()))).await.is_err() {
                                    let _ = ch.close().await;
                                    return;
                                }
                            }
                            russh::ChannelMsg::Close => break,
                            _ => {}
                        }
                    }
                    let _ = tx.send((idx, None)).await;
                })
            }
        };
        producers.push(h.abort_handle());
    }
    drop(tx);

    let names: Vec<String> = containers.into_iter().map(|(_, n)| n).collect();
    let keep = b.clone();
    let collector = tokio::spawn(async move {
        let _keep = keep;
        let _guards: Vec<AbortOnDrop> = producers.into_iter().map(AbortOnDrop).collect();
        let n = names.len();
        let mut bufs: Vec<Vec<u8>> = vec![Vec::new(); n];
        let mut last_key: Vec<Vec<u8>> = vec![Vec::new(); n];
        let mut open = n;
        let prefix = |i: usize| {
            let name: String = names[i].chars().take(width).collect();
            format!("\x1b[{}m{:<w$} |\x1b[0m ", COLORS[i % COLORS.len()], name, w = width)
        };
        // The tail of every container arrives in a burst: collect it briefly
        // and sort it by time, then pass new lines through as they come.
        let mut backlog: Option<Vec<(Vec<u8>, Vec<u8>)>> = Some(Vec::new());
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(800);
        let flush = |backlog: &mut Option<Vec<(Vec<u8>, Vec<u8>)>>| -> Vec<u8> {
            let mut lines = backlog.take().unwrap_or_default();
            lines.sort_by(|a, b| a.0.cmp(&b.0)); // stable: equal times keep arrival order
            lines.into_iter().flat_map(|(_, l)| l).collect()
        };
        loop {
            let msg = if backlog.is_some() {
                tokio::select! {
                    m = rx.recv() => m,
                    _ = tokio::time::sleep_until(deadline) => {
                        let out_bytes = flush(&mut backlog);
                        if !out_bytes.is_empty() && out.send(data_event(&out_bytes)).is_err() {
                            return;
                        }
                        continue;
                    }
                }
            } else {
                rx.recv().await
            };
            let Some((i, chunk)) = msg else { break };
            let mut lines: Vec<Vec<u8>> = Vec::new();
            match &chunk {
                Some(bytes) => {
                    bufs[i].extend_from_slice(bytes);
                    while let Some(pos) = bufs[i].iter().position(|&c| c == b'\n') {
                        lines.push(bufs[i].drain(..=pos).collect());
                    }
                }
                None => {
                    if !bufs[i].is_empty() {
                        lines.push(std::mem::take(&mut bufs[i]));
                    }
                    open -= 1;
                }
            }
            let ended = chunk.is_none();
            let mut rendered: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for line in lines {
                let line = line.strip_suffix(b"\n").unwrap_or(&line);
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                let (ts, rest) = split_timestamp(line);
                if let Some(ts) = ts {
                    last_key[i] = sort_key(ts);
                }
                let mut l = prefix(i).into_bytes();
                l.extend_from_slice(if timestamps { line } else { rest });
                l.extend_from_slice(b"\r\n");
                rendered.push((last_key[i].clone(), l));
            }
            if ended {
                let mut l = prefix(i).into_bytes();
                l.extend_from_slice(b"\x1b[90m(stream ended)\x1b[0m\r\n");
                rendered.push((last_key[i].clone(), l));
            }
            match backlog.as_mut() {
                Some(b) => b.extend(rendered),
                None => {
                    let out_bytes: Vec<u8> = rendered.into_iter().flat_map(|(_, l)| l).collect();
                    if !out_bytes.is_empty() && out.send(data_event(&out_bytes)).is_err() {
                        return;
                    }
                }
            }
            if open == 0 {
                break;
            }
        }
        let tail_bytes = flush(&mut backlog);
        if !tail_bytes.is_empty() {
            let _ = out.send(data_event(&tail_bytes));
        }
        let _ = out.send(TermEvent::Exit { code: None });
    });
    app.state::<AppState>().add_task(task_id.clone(), collector.abort_handle());
    Ok(task_id)
}

/// Split Docker's `--timestamps` prefix (`2026-11-10T05:11:13.123456789Z `)
/// off a log line. Lines without one are returned unchanged.
fn split_timestamp(line: &[u8]) -> (Option<&[u8]>, &[u8]) {
    let Some(sp) = line.iter().position(|&c| c == b' ') else { return (None, line) };
    let ts = &line[..sp];
    let looks_like = ts.len() >= 20 && ts[0].is_ascii_digit() && ts.get(10) == Some(&b'T') && ts.ends_with(b"Z");
    if looks_like {
        (Some(ts), &line[sp + 1..])
    } else {
        (None, line)
    }
}

/// Sortable form of an RFC 3339 UTC timestamp: Docker trims trailing zeros of
/// the fraction (".1Z" vs ".12Z"), so pad it to 9 digits first.
fn sort_key(ts: &[u8]) -> Vec<u8> {
    let body = ts.strip_suffix(b"Z").unwrap_or(ts);
    let (secs, frac) = match body.iter().position(|&c| c == b'.') {
        Some(p) => (&body[..p], &body[p + 1..]),
        None => (body, &b""[..]),
    };
    let mut k = secs.to_vec();
    k.push(b'.');
    k.extend_from_slice(&frac[..frac.len().min(9)]);
    k.resize(secs.len() + 1 + 9, b'0');
    k
}

/// Aborts the producer tasks when the collector is cancelled or finishes.
struct AbortOnDrop(tokio::task::AbortHandle);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_timestamps_split_and_sort() {
        let (ts, rest) = split_timestamp(b"2026-11-10T05:11:13.1Z GET /api 200");
        assert_eq!(ts, Some(&b"2026-11-10T05:11:13.1Z"[..]));
        assert_eq!(rest, b"GET /api 200");
        assert_eq!(split_timestamp(b"plain line here"), (None, &b"plain line here"[..]));
        assert_eq!(split_timestamp(b"nospace"), (None, &b"nospace"[..]));
        // .12 is later than .1 even though it sorts first as raw text.
        assert!(sort_key(b"2026-11-10T05:11:13.12Z") > sort_key(b"2026-11-10T05:11:13.1Z"));
        assert!(sort_key(b"2026-11-10T05:11:14Z") > sort_key(b"2026-11-10T05:11:13.999999999Z"));
    }

    #[test]
    fn cli_parsers() {
        let out = "Deleted Volumes:\nabc\ndef\n\nTotal reclaimed space: 1.5GB\n";
        assert_eq!(reclaimed_from_cli(out).as_deref(), Some("1.5GB"));
        assert_eq!(cli_removed_lines(out), vec!["abc", "def"]);
        assert_eq!(label_from_cli("a=1,com.docker.compose.project=web,b=2", PROJECT_LABEL).as_deref(), Some("web"));
        assert!(valid_name("my_vol-1.x", "").is_ok());
        assert!(valid_name("bad name", "").is_err());
        assert!(valid_name("ghcr.io/org/img:1.2@sha256:abc", ":/@").is_ok());
        assert!(valid_name("x;rm -rf", ":/@").is_err());
        assert_eq!(registry_of("nginx:alpine"), "docker.io");
        assert_eq!(registry_of("library/nginx"), "docker.io");
        assert_eq!(registry_of("ghcr.io/org/app:1"), "ghcr.io");
        assert_eq!(registry_of("localhost:5000/app"), "localhost:5000");
    }
}

/// Opt-in check against the local Docker daemon (non-destructive except for
/// its own `shellport-it-*` objects and restarting the `dev` compose project):
///   cargo test --lib docker_admin::live -- --ignored --nocapture
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn local_daemon_roundtrip() {
        let docker = bollard::Docker::connect_with_local_defaults().unwrap();
        let b = DockerBackend::Api { docker, _bridge: None };

        let df = b.disk_usage().await.unwrap();
        println!("df: {:?}", df.iter().map(|r| (&r.kind, r.total, r.size)).collect::<Vec<_>>());
        assert!(df.iter().any(|r| r.kind == "Images"));

        b.volume_create("shellport-it-vol").await.unwrap();
        let vols = b.volumes().await.unwrap();
        assert!(vols.iter().any(|v| v.name == "shellport-it-vol" && v.used_by.is_empty()));
        println!("volumes: {} ({} in use)", vols.len(), vols.iter().filter(|v| !v.used_by.is_empty()).count());
        b.volume_inspect("shellport-it-vol").await.unwrap();
        b.volume_remove("shellport-it-vol", false).await.unwrap();

        b.network_create("shellport-it-net", true).await.unwrap();
        let nets = b.networks().await.unwrap();
        let n = nets.iter().find(|n| n.name == "shellport-it-net").unwrap();
        assert!(n.internal && !n.builtin);
        assert!(nets.iter().any(|n| n.name == "bridge" && n.builtin));
        println!("networks: {:?}", nets.iter().map(|n| (&n.name, n.containers.len())).collect::<Vec<_>>());
        b.network_remove(&n.id).await.unwrap();

        let dev = project_containers(&b, "dev").await.unwrap();
        println!("dev project: {:?}", dev.iter().map(|c| &c.1).collect::<Vec<_>>());
        assert!(!dev.is_empty());
        let n = project_action(&b, "dev", Action::Restart).await.unwrap();
        assert_eq!(n, dev.len());
        assert!(project_action(&b, "dev", Action::Remove).await.is_err());
    }
}
