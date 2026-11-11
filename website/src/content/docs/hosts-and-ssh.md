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

Hosts imported from `~/.ssh/config` stay **in sync** with it (*Keep in sync with ~/.ssh/config* in the host editor):

- Changes you make in the file show up when Shellport regains focus.
- Saving a host in Shellport rewrites only that host's `HostName`, `User`, `Port`, `IdentityFile` and `ProxyJump` lines. Comments, indentation, other settings and other hosts are left exactly as they were, and the previous file is kept as `config.shellport-backup`.
- Passwords and vault keys stay in Shellport; the file never gets secrets.
- Hosts defined in an `Include`d file are read but not written; edit them in that file.

## Import and export as CSV or JSON

In the import dialog, **Import a file…** accepts a Shellport JSON export or a CSV with a `hostname` column (`name`, `user`, `port`, `group`, `key_file`, `jump_host`, `tags` and `production` are optional). **Export all hosts** writes JSON or CSV. Exports never contain passwords or keys.

## Production servers, notes and status

- Tick **Production server** to mark a host: it shows a PROD badge in the sidebar, on its tabs and in the palette, and databases reached through it treat risky statements as *Confirm risky* even when set to *Off*.
- **Notes** (the notebook icon on a host) hold Markdown. Code blocks have **Copy** and **Paste in terminal**; pasting never presses Enter for you.
- The dot on a host's icon shows whether its SSH port answers. Shellport checks every few minutes while the window is visible, by sending a short SSH greeting, like `ssh-keyscan`. Turn it off in Settings if your servers' intrusion-prevention tools object.
- **Ctrl+Shift+P** opens the command palette for any host, database, tab or action. (Ctrl+K is left alone: in a terminal it deletes to the end of the line.)
