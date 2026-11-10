---
title: Hosts & SSH terminals
description: Save hosts in groups, choose how to sign in, and work in tabbed terminals.
section: Features
order: 10
---

## Terminals

- Tabbed **SSH** and **local** terminals with search and four colour themes.
- **Hide the sidebar** with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>B</kbd> when you want the whole window for the terminal.
- Links in terminal output open in your browser only after you confirm, and only `http(s)` links.

## When the connection drops

After sleep, a Wi-Fi change or a server restart, SSH terminals **reconnect on their own**, waiting a little longer between attempts (1, 2, 5, 10, then every 30 seconds) and pausing while you're offline. A banner shows the countdown with **Reconnect now** and **Stop**. Each host can instead **ask** before reconnecting or **close the tab**: see *When the connection drops* in the host editor.

### Keep work running with tmux

Tick **Keep sessions alive with tmux** on a host and each terminal tab runs inside its own tmux session on the server. If the connection drops, your programs keep running, and reconnecting (or reopening Shellport) picks up exactly where you were. Closing the tab ends its tmux session. The server needs `tmux` installed; without it you get a normal shell and a short notice.

## Saved hosts

Save hosts in **groups** (Production, Staging, …) with colours and tags. Each host can have a **startup command**, for example `cd /srv/app`.

## Signing in

Shellport supports four ways to authenticate:

- a **password**,
- a **key stored in the vault**,
- a **key file** on disk,
- **ssh-agent**.

**Two-factor (keyboard-interactive) logins** such as one-time codes are supported too.

## Jump hosts

Use **ProxyJump** to reach private servers through a bastion. Pick the jump host in the host editor; Shellport chains the connection for you.

## Import from ~/.ssh/config

Click the **import** icon in the Hosts panel. Shellport understands `Include`, `Host *` defaults and `ProxyJump`, so most setups come across with no extra work.
