//! Opening and tracking database sessions: vault password, optional SSH
//! tunnel, then the engine's driver.

use super::{registry, ConnectParams, DbSession};
use crate::state::AppState;
use crate::store::databases::DbProfile;
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

/// Explain why the SSH server refused to open the tunnel, in plain words.
fn unreachable_message(via: &str, host: &str, port: u16, err: &str) -> String {
    let why = if err.contains("AdministrativelyProhibited") {
        "TCP forwarding is disabled on that server (AllowTcpForwarding in sshd_config)".to_string()
    } else if err.contains("ConnectFailed") {
        "the host name did not resolve, or nothing is listening on that port".to_string()
    } else {
        err.to_string()
    };
    format!("{via} could not reach {host}:{port}: {why}.\nCheck the host and port as {via} sees them.")
}

pub fn tunnel_task_id(session_id: &str) -> String {
    format!("dbtun:{session_id}")
}

/// Read the password while holding the vault lock only briefly. The error
/// text matches the SSH one so the UI shows its unlock prompt.
fn resolve_password(state: &AppState, db: &DbProfile) -> Result<Option<Zeroizing<String>>> {
    let Some(id) = db.password_secret_id.as_deref() else { return Ok(None) };
    let mut vault = state.vault.lock();
    if !vault.is_unlocked() {
        return Err(anyhow!("unlock the vault to use the stored password for {}", db.name));
    }
    vault.touch();
    Ok(Some(Zeroizing::new(vault.get(id)?.value.clone())))
}

/// Connect a profile. Returns the live session and, when tunnelling, the id
/// of the tunnel task (cancel it when the session closes).
pub async fn open(app: &AppHandle, db: &DbProfile, session_id: &str) -> Result<(Arc<dyn DbSession>, Option<String>)> {
    let state = app.state::<AppState>();
    let password = resolve_password(&state, db)?;
    let (connect_host, connect_port, tunnel) = match &db.via_host_id {
        Some(host_id) => {
            // Connect first so SSH auth problems are reported as such.
            let ssh = crate::ssh::get_conn(app.clone(), host_id.clone(), 0).await.context("SSH tunnel")?;
            // Check the SSH server can reach the database before handing a
            // local port to the driver, which would only see "connection reset".
            match ssh.handle.channel_open_direct_tcpip(db.host.clone(), db.port as u32, "127.0.0.1", 0).await {
                Ok(ch) => {
                    let _ = ch.close().await;
                }
                Err(e) => return Err(anyhow!(unreachable_message(&ssh.host.name, &db.host, db.port, &format!("{e:?}")))),
            }
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
            let port = listener.local_addr()?.port();
            let h = crate::ssh::spawn_forward_loop(app.clone(), host_id.clone(), listener, db.host.clone(), db.port);
            let task = tunnel_task_id(session_id);
            state.add_task(task.clone(), h.abort_handle());
            ("127.0.0.1".to_string(), port, Some(task))
        }
        None => (db.host.clone(), db.port, None),
    };
    let params = ConnectParams {
        host: db.host.clone(),
        connect_host,
        connect_port,
        database: db.database.clone(),
        username: db.username.clone(),
        password,
        ssl_mode: db.ssl_mode,
        options: db.options.clone(),
        read_only: db.safety == crate::store::databases::Safety::ReadOnly,
    };
    match registry::driver(db.engine).connect(params).await {
        Ok(s) => Ok((s, tunnel)),
        Err(e) => {
            if let Some(t) = &tunnel {
                state.cancel_task(t);
            }
            let via = db.via_host_id.as_ref().map(|_| " (through SSH)").unwrap_or("");
            Err(e.context(format!("connecting to {}:{}{via}", db.host, db.port)))
        }
    }
}

/// A connected session plus what to clean up when it closes.
pub struct OpenDb {
    pub session: Arc<dyn DbSession>,
    pub profile_id: String,
    pub tunnel_task: Option<String>,
    pub safety: crate::store::databases::Safety,
}

pub async fn close(state: &AppState, session_id: &str) {
    let removed = state.db_sessions.lock().await.remove(session_id);
    if let Some(o) = removed {
        o.session.close().await;
        if let Some(t) = &o.tunnel_task {
            state.cancel_task(t);
        }
    }
}

/// A live session, or a `connection lost` error the UI offers to reconnect.
pub async fn get(state: &AppState, session_id: &str) -> Result<(Arc<dyn DbSession>, crate::store::databases::Safety)> {
    let s = state
        .db_sessions
        .lock()
        .await
        .get(session_id)
        .map(|o| (o.session.clone(), o.safety))
        .ok_or_else(|| anyhow!("{}: the session was closed", super::CONNECTION_LOST))?;
    if s.0.is_closed() {
        return Err(anyhow!("{}: the server closed the connection or the network dropped", super::CONNECTION_LOST));
    }
    Ok(s)
}

/// Turn a driver error into `connection lost` when the connection died
/// during the call, so the UI can offer to reconnect.
pub fn map_lost(session: &dyn DbSession, e: anyhow::Error) -> anyhow::Error {
    if session.is_closed() {
        anyhow!("{}: {e:#}", super::CONNECTION_LOST)
    } else {
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_reasons_are_readable() {
        let m = unreachable_message("dev-box", "db.internal", 5432, "ChannelOpenFailure(ConnectFailed)");
        assert!(m.starts_with("dev-box could not reach db.internal:5432: the host name did not resolve"), "{m}");
        let m = unreachable_message("bastion", "db", 5432, "ChannelOpenFailure(AdministrativelyProhibited)");
        assert!(m.contains("AllowTcpForwarding"), "{m}");
    }
}
