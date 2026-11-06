//! Headless smoke test of the protocol paths Shellport relies on, against the
//! disposable sshd from dev/docker-compose.test.yml:
//!   cargo run --example smoke
use std::sync::Arc;
use std::time::Duration;

use bollard::query_parameters::ListContainersOptionsBuilder;
use russh::client;
use russh::keys::PublicKeyOrCertificate;

struct H;
impl client::Handler for H {
    type Error = anyhow::Error;
    async fn check_server_key(&mut self, _k: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Arc::new(client::Config::default());
    let mut h = client::connect(cfg, ("127.0.0.1", 2222), H).await?;
    assert!(h.authenticate_password("test", "shellport-test").await?.success(), "auth failed");
    println!("[ok] password auth");

    // PTY shell round trip
    let ch = h.channel_open_session().await?;
    ch.request_pty(false, "xterm-256color", 80, 24, 0, 0, &[]).await?;
    ch.request_shell(true).await?;
    let (mut r, w) = ch.split();
    w.data(&b"echo shellport-$((40+2)); exit\n"[..]).await?;
    let mut out = Vec::new();
    while let Ok(Some(m)) = tokio::time::timeout(Duration::from_secs(5), r.wait()).await {
        if let russh::ChannelMsg::Data { data } = m {
            out.extend_from_slice(&data);
        }
    }
    assert!(String::from_utf8_lossy(&out).contains("shellport-42"), "shell output: {}", String::from_utf8_lossy(&out));
    println!("[ok] pty shell");

    // SFTP
    let ch = h.channel_open_session().await?;
    ch.request_subsystem(true, "sftp").await?;
    let sftp = russh_sftp::client::SftpSession::new(ch.into_stream()).await?;
    let home = sftp.canonicalize(".").await?;
    {
        use tokio::io::AsyncWriteExt;
        let mut f = sftp
            .open_with_flags(format!("{home}/smoke.txt"), russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::TRUNCATE | russh_sftp::protocol::OpenFlags::WRITE)
            .await?;
        f.write_all(b"hello").await?;
        f.shutdown().await?;
    }
    let back = sftp.read(format!("{home}/smoke.txt")).await?;
    assert_eq!(back, b"hello");
    sftp.remove_file(format!("{home}/smoke.txt")).await?;
    println!("[ok] sftp write/read/remove in {home}");

    // Docker over streamlocal bridge
    let h = Arc::new(h);
    let dir = tempfile::tempdir()?;
    let sock = dir.path().join("d.sock");
    let listener = tokio::net::UnixListener::bind(&sock)?;
    let hh = h.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { break };
            let hh = hh.clone();
            tokio::spawn(async move {
                match hh.channel_open_direct_streamlocal("/var/run/docker.sock").await {
                    Ok(ch) => {
                        let mut cs = ch.into_stream();
                        let _ = tokio::io::copy_bidirectional(&mut s, &mut cs).await;
                    }
                    Err(e) => eprintln!("streamlocal: {e}"),
                }
            });
        }
    });
    let docker = bollard::Docker::connect_with_unix(sock.to_str().unwrap(), 30, bollard::API_DEFAULT_VERSION)?;
    let v = docker.version().await?;
    let list = docker.list_containers(Some(ListContainersOptionsBuilder::new().all(true).build())).await?;
    println!("[ok] docker via ssh socket: server {:?}, {} containers", v.version, list.len());
    Ok(())
}
