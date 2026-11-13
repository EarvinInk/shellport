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

## Still stuck?

Ask on the [contact page](../../contact/) and include your distribution, Shellport version and what you tried.
