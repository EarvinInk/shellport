//! Run one command on many hosts and collect a result per host (M2.3,
//! "run and collect"). Up to 8 hosts at once; each run has a timeout and its
//! output is capped. The whole run can be cancelled, and the outcome goes to
//! Activity.

use crate::activity::{ActivityEntry, ActivityKind, HostResult};
use crate::state::AppState;
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

const PARALLEL: usize = 8;
const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    Started { host_id: String },
    Done { result: HostResult },
    #[serde(rename_all = "camelCase")]
    Finished { ok: usize, failed: usize, activity_id: String },
}

/// Exit status and combined output of one command; output is capped.
pub async fn exec_collect(conn: &crate::ssh::SshConn, cmd: &str) -> Result<(Option<u32>, String)> {
    let mut ch = conn.handle.channel_open_session().await?;
    ch.exec(true, cmd).await?;
    let mut out = Vec::new();
    let mut code = None;
    let mut truncated = false;
    while let Some(msg) = ch.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                if out.len() < MAX_OUTPUT {
                    out.extend_from_slice(&data[..data.len().min(MAX_OUTPUT - out.len())]);
                } else {
                    truncated = true;
                }
            }
            russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
            russh::ChannelMsg::Close => break,
            _ => {}
        }
    }
    let mut text = String::from_utf8_lossy(&out).into_owned();
    if truncated {
        text.push_str("\n… output cut off at 64 KB");
    }
    Ok((code, text))
}

/// `{{host}}`, `{{user}}` and `{{name}}` filled in for one host.
pub fn per_host(cmd: &str, host: &crate::store::hosts::Host) -> String {
    cmd.replace("{{host}}", &host.hostname).replace("{{user}}", &host.username).replace("{{name}}", &host.name)
}

/// "web-1, web-2, db-1 +4": which hosts a run touched.
fn hosts_label(results: &[HostResult]) -> String {
    let mut names: Vec<&str> = results.iter().map(|r| r.host_name.as_str()).collect();
    names.sort_unstable();
    let more = names.len().saturating_sub(3);
    let shown = names[..names.len().min(3)].join(", ");
    if more > 0 { format!("{shown} +{more}") } else { shown }
}

pub fn task_id(run_id: &str) -> String {
    format!("run:{run_id}")
}

/// Start a run; returns at once with the run id (cancel with `task_cancel("run:<id>")`).
/// Production hosts must be acknowledged explicitly (app rule 2).
pub fn start(app: AppHandle, host_ids: Vec<String>, command: String, timeout_secs: u64, confirmed_production: bool, out: Channel<RunEvent>) -> Result<String> {
    if command.trim().is_empty() {
        bail!("the command is empty");
    }
    if host_ids.is_empty() {
        bail!("choose at least one host");
    }
    let state = app.state::<AppState>();
    let hosts: Vec<crate::store::hosts::Host> = {
        let store = state.hosts.lock();
        host_ids.iter().map(|id| store.get(id)).collect::<Result<_>>()?
    };
    let prod: Vec<&str> = hosts.iter().filter(|h| h.production).map(|h| h.name.as_str()).collect();
    if !prod.is_empty() && !confirmed_production {
        return Err(anyhow!("this run includes production hosts ({}); confirm to continue", prod.join(", ")));
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let timeout = Duration::from_secs(timeout_secs.clamp(1, 3600));
    let app2 = app.clone();
    let cmd = command.clone();
    let tid = task_id(&run_id);
    let h = tokio::spawn(async move {
        let started = Instant::now();
        let results: Vec<HostResult> = futures_util::stream::iter(hosts.into_iter().map(|host| {
            let (app, cmd, out) = (app2.clone(), cmd.clone(), out.clone());
            async move {
                let _ = out.send(RunEvent::Started { host_id: host.id.clone() });
                let t0 = Instant::now();
                let cmd = per_host(&cmd, &host);
                let res = tokio::time::timeout(timeout, async {
                    let conn = crate::ssh::get_conn(app, host.id.clone(), 0).await?;
                    exec_collect(&conn, &cmd).await
                })
                .await;
                let (ok, exit_code, output) = match res {
                    Ok(Ok((code, text))) => (code == Some(0), code, text),
                    Ok(Err(e)) => (false, None, format!("{e:#}")),
                    Err(_) => (false, None, format!("timed out after {}s", timeout.as_secs())),
                };
                let r = HostResult { host_id: host.id, host_name: host.name, ok, exit_code, output, duration_ms: t0.elapsed().as_millis() as u64 };
                let _ = out.send(RunEvent::Done { result: r.clone() });
                r
            }
        }))
        .buffer_unordered(PARALLEL)
        .collect()
        .await;
        let ok = results.iter().filter(|r| r.ok).count();
        let failed = results.len() - ok;
        let mut e = ActivityEntry::new(ActivityKind::MultiRun, hosts_label(&results), &cmd, failed == 0);
        e.duration_ms = Some(started.elapsed().as_millis() as u64);
        e.count = Some(results.len() as u64);
        e.results = results.into_iter().map(|mut r| (r.output = crate::activity::clip(&r.output), r).1).collect();
        let id = e.id.clone();
        app2.state::<AppState>().activity.add(e);
        let _ = out.send(RunEvent::Finished { ok, failed, activity_id: id });
        app2.state::<AppState>().tasks.lock().remove(&tid);
    });
    state.add_task(task_id(&run_id), h.abort_handle());
    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(name: &str) -> HostResult {
        HostResult { host_id: name.into(), host_name: name.into(), ok: true, exit_code: Some(0), output: String::new(), duration_ms: 0 }
    }

    #[test]
    fn labels_and_placeholders() {
        let rs: Vec<_> = ["web-2", "db-1", "web-1", "cache", "web-3"].map(result).into();
        assert_eq!(hosts_label(&rs), "cache, db-1, web-1 +2");
        assert_eq!(hosts_label(&rs[..1]), "web-2");
        let mut h: crate::store::hosts::Host = serde_json::from_value(serde_json::json!({
            "id": "h", "name": "web-1", "hostname": "10.0.0.5", "port": 22, "username": "deploy", "auth": {"type": "agent"}
        }))
        .unwrap();
        assert_eq!(per_host("ssh {{user}}@{{host}} # {{name}} {{other}}", &h), "ssh deploy@10.0.0.5 # web-1 {{other}}");
        h.name = "x".into();
        assert_eq!(per_host("{{name}}", &h), "x");
    }
}
