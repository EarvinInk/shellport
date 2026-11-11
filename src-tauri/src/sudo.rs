//! Working as root on a host (M3.3): an SFTP session run through `sudo`, and
//! root commands for compress/extract. The sudo password is asked for each
//! new session, sent once to sudo, and never stored.

use crate::ssh::SshConn;
use crate::state::AppState;
use anyhow::{bail, Context, Result};
use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use russh_sftp::client::SftpSession;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

/// Prefix of the error returned when sudo needs a password (the UI then asks for one).
pub const NEED_PASSWORD: &str = "sudo password needed:";
/// Printed on stderr by the root shell once sudo let it through.
const MARK: &str = "SHELLPORT_SUDO_OK_7f3a";

/// Quote one argument for a POSIX shell.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Ok,
    NeedPassword,
    WrongPassword,
    Failed(String),
}

/// What sudo's stderr so far says, if anything final.
fn classify(stderr: &str) -> Option<Outcome> {
    if stderr.contains(MARK) {
        return Some(Outcome::Ok);
    }
    let low = stderr.to_lowercase();
    if low.contains("a password is required") || low.contains("a terminal is required") {
        return Some(Outcome::NeedPassword);
    }
    if low.contains("sorry, try again") || low.contains("incorrect password") {
        return Some(Outcome::WrongPassword);
    }
    for bad in ["not in the sudoers", "is not allowed to run sudo", "may not run sudo", "sudo: not found", "sudo: command not found", "account validation failure"] {
        if low.contains(bad) {
            let line = stderr.lines().find(|l| l.to_lowercase().contains(bad)).unwrap_or(stderr).trim();
            return Some(Outcome::Failed(line.to_string()));
        }
    }
    None
}

/// The shell command that runs `inner` as root and prints the marker first.
fn sudo_command(inner: &str, with_password: bool) -> String {
    let script = format!("echo {MARK} >&2; {inner}");
    if with_password {
        format!("sudo -S -p '' -- sh -c {}", quote(&script))
    } else {
        format!("sudo -n -- sh -c {}", quote(&script))
    }
}

/// Start `inner` as root on a new exec channel. Returns the channel once sudo
/// has let it through; anything the command writes comes after that.
async fn start(conn: &SshConn, inner: &str, password: Option<&str>) -> Result<Channel<Msg>> {
    let mut ch = conn.handle.channel_open_session().await?;
    ch.exec(true, sudo_command(inner, password.is_some())).await?;
    if let Some(pw) = password {
        let line = Zeroizing::new(format!("{pw}\n"));
        ch.data(line.as_bytes()).await?;
    }
    let mut err = String::new();
    let wait = async {
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::ExtendedData { data, .. } => {
                    err.push_str(&String::from_utf8_lossy(&data));
                    match classify(&err) {
                        Some(Outcome::Ok) => return Ok(()),
                        Some(Outcome::NeedPassword) => bail!("{NEED_PASSWORD} sudo on this host asks for a password"),
                        Some(Outcome::WrongPassword) => bail!("sudo: wrong password"),
                        Some(Outcome::Failed(m)) => bail!("{m}"),
                        None => {}
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    let msg = err.trim();
                    if exit_status == 127 && msg.is_empty() {
                        bail!("sudo isn't installed on this host");
                    }
                    bail!("sudo failed ({exit_status}){}", if msg.is_empty() { String::new() } else { format!(": {msg}") });
                }
                ChannelMsg::Eof | ChannelMsg::Close => bail!("sudo ended without starting the command{}", if err.trim().is_empty() { String::new() } else { format!(": {}", err.trim()) }),
                _ => {}
            }
        }
        bail!("the connection closed while waiting for sudo")
    };
    match tokio::time::timeout(Duration::from_secs(20), wait).await {
        Ok(Ok(())) => Ok(ch),
        Ok(Err(e)) => {
            let _ = ch.close().await;
            Err(e)
        }
        Err(_) => {
            let _ = ch.close().await;
            bail!("sudo didn't answer within 20 seconds")
        }
    }
}

/// Where the SFTP server program lives on this host.
async fn sftp_server_path(conn: &SshConn) -> Result<String> {
    let out = conn
        .exec_capture(
            "for p in /usr/lib/openssh/sftp-server /usr/libexec/openssh/sftp-server /usr/lib/ssh/sftp-server /usr/libexec/sftp-server /usr/lib/sftp-server /usr/sbin/sftp-server; do if [ -x \"$p\" ]; then echo \"$p\"; exit 0; fi; done; command -v sftp-server || true",
        )
        .await?;
    match out.lines().map(str::trim).find(|l| l.starts_with('/')) {
        Some(p) => Ok(p.to_string()),
        None => bail!("couldn't find the sftp-server program on this host, which browsing as root needs"),
    }
}

/// Open (or reopen) the root SFTP session for a host.
pub async fn open_sftp(app: &AppHandle, host_id: &str, password: Option<Zeroizing<String>>) -> Result<()> {
    let conn = crate::ssh::get_conn(app.clone(), host_id.to_string(), 0).await?;
    let path = sftp_server_path(&conn).await?;
    let ch = start(&conn, &format!("exec {}", quote(&path)), password.as_deref().map(|s| s.as_str())).await?;
    drop(password);
    let s = SftpSession::new(ch.into_stream()).await.context("starting SFTP as root")?;
    app.state::<AppState>().sudo_sftp.lock().await.insert(host_id.to_string(), Arc::new(s));
    Ok(())
}

pub async fn close_sftp(app: &AppHandle, host_id: &str) {
    if let Some(s) = app.state::<AppState>().sudo_sftp.lock().await.remove(host_id) {
        let _ = s.close().await;
    }
}

/// The open root SFTP session for a host.
pub async fn sftp(app: &AppHandle, host_id: &str) -> Result<Arc<SftpSession>> {
    let state = app.state::<AppState>();
    let s = state.sudo_sftp.lock().await.get(host_id).cloned();
    match s {
        Some(s) if s.canonicalize(".").await.is_ok() => Ok(s),
        _ => {
            state.sudo_sftp.lock().await.remove(host_id);
            bail!("{NEED_PASSWORD} the root session ended; turn sudo on again")
        }
    }
}

/// Run a shell command, as root when `sudo` is set; returns (exit code, output).
pub async fn run(conn: &SshConn, cmd: &str, sudo: bool, password: Option<Zeroizing<String>>) -> Result<(Option<u32>, String)> {
    let mut ch = if sudo {
        start(conn, cmd, password.as_deref().map(|s| s.as_str())).await?
    } else {
        let ch = conn.handle.channel_open_session().await?;
        ch.exec(true, cmd.to_string()).await?;
        ch
    };
    drop(password);
    let mut out = Vec::new();
    let mut code = None;
    while let Some(msg) = ch.wait().await {
        match msg {
            // Keep the first 256 KB of output.
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } if out.len() < 256 * 1024 => out.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok((code, String::from_utf8_lossy(&out).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_hostile_names() {
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote("$(rm -rf /); `x`"), "'$(rm -rf /); `x`'");
        // Round-trip through a real shell: the argument arrives unchanged.
        for s in ["a b", "it's", "$(id)", "-rf", "x\ny", "\"q\"", "back\\slash"] {
            let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s {}", quote(s))).output().unwrap();
            assert_eq!(String::from_utf8(out.stdout).unwrap(), s);
        }
    }

    #[test]
    fn reads_sudo_answers() {
        assert_eq!(classify(&format!("{MARK}\n")), Some(Outcome::Ok));
        assert_eq!(classify("sudo: a password is required\n"), Some(Outcome::NeedPassword));
        assert_eq!(classify("Sorry, try again.\n"), Some(Outcome::WrongPassword));
        assert_eq!(classify("sudo: 1 incorrect password attempt\n"), Some(Outcome::WrongPassword));
        assert!(matches!(classify("test is not in the sudoers file.  This incident will be reported.\n"), Some(Outcome::Failed(_))));
        assert_eq!(classify("partial out"), None);
    }

    #[test]
    fn command_shape() {
        let c = sudo_command("exec '/usr/lib/ssh/sftp-server'", false);
        assert!(c.starts_with("sudo -n -- sh -c '"));
        assert!(c.contains(MARK));
        assert!(sudo_command("true", true).starts_with("sudo -S -p '' -- sh -c "));
    }
}
