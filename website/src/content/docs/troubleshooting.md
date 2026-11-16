---
title: Troubleshooting
description: Fixes for the most common problems.
section: Reference
order: 22
---

## The AppImage won't start

AppImages need FUSE 2. Install it with:

```bash
sudo apt install libfuse2t64   # Ubuntu 24.04+
sudo apt install libfuse2      # Ubuntu 22.04 / Debian
```

## "Host key changed" warning

The server's identity no longer matches `~/.ssh/known_hosts`. Shellport blocks the connection and sends no secrets. If you know the server was reinstalled or its keys rotated, confirm the new fingerprint with the server's administrator before replacing the saved key.

## I forgot my vault master password

The vault can't be recovered without it. Delete `~/.config/shellport/vault.bin` to start a new, empty vault. Your hosts and settings are kept.

## Remote Docker shows nothing

Make sure the SSH user can access Docker on that server (for example, is in the `docker` group) and that Docker is running there.

## A database connection through SSH fails

With an **SSH tunnel** set, the database host and port are resolved *from that server*. If PostgreSQL only listens locally on the server, use host `127.0.0.1`, not the server's public address. Use **Test connection** to see the error, and check you can open an SSH terminal to the same host first.

## Errors behind PgBouncer or Supabase

Errors about prepared statements usually mean a connection pooler in transaction mode. Tick **Behind a connection pooler** in the connection's Advanced section, or add `pgbouncer=true` to a pasted connection string.

## Programs stop when the connection drops

That's how plain SSH works: the server ends the session's programs. Tick **Keep sessions alive with tmux** on the host (the server needs `tmux` installed), and reconnecting picks up where you were.

## A server's intrusion protection blocks me

Some tools (fail2ban and similar) dislike the short SSH greetings behind the up/down dots. Turn the status checks off in Settings.

## `trz` / `tsz` do nothing

trzsz must be installed on the server: `pip install trzsz`, or your distribution's package.

## Changes to ~/.ssh/config don't show up

Shellport rereads the file when its window regains focus. Hosts defined in an `Include`d file are read but not written: edit those in that file.

## Still stuck?

Ask on the [contact page](../../contact/) and include your distribution, Shellport version and what you tried.
