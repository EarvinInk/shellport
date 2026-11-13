---
title: Work across servers
description: Split panes, workspaces, saved commands, running commands on many hosts, and the activity history.
section: Features
order: 10.5
---

## Split panes

Put several terminals in one tab, side by side or stacked. Use **Split right** and **Split down** in a pane's corner, or the command palette, which also has *Open host in a split*. Drag the divider to resize, and close a pane from its corner.

With *Restore tabs on startup* on, splits come back with your tabs. On hosts using [tmux](../hosts-and-ssh/#keep-work-running-with-tmux), each pane picks up exactly where it was.

## Workspaces

A workspace remembers your open terminal tabs and how they're split. Open **Commands library** in the sidebar, type a name under *Workspaces* and click **Save open terminals**. Reopen it from there, or with *Open workspace* in the palette.

## Commands library

Save commands you use often in **Commands library** (sidebar). Each command can apply to all hosts or to chosen hosts and groups.

- Use `{{name}}` or `{{name:default}}` for values you fill in each time, for example `sudo systemctl restart {{service:nginx}}`. `{{host}}`, `{{user}}` and `{{name}}` are filled in from the host.
- **Paste into the active terminal** types the command without pressing Enter, so you can check it first.
- **Run in new terminals on these hosts** types the command once after login in each new terminal (not after a reconnect, and not on tmux hosts, where the session already ran it).
- Saved commands appear in the palette as *Paste: …* and *Run on hosts: …*.

## Broadcast typing

Click the **broadcast** button in a pane's corner (or *Broadcast typing to all panes* in the palette) to send what you type to every pane in the tab. A banner shows while it's on. Panes can leave and rejoin the broadcast one by one. If a production host is in the tab, Shellport asks first.

## Run on many hosts

**Run on several hosts** in the sidebar (or *Run a command on several hosts…* in the palette) runs one command on the hosts you pick, in parallel:

1. Pick the hosts, filtering by name if you have many.
2. Type a command or choose a saved one, and set a time limit per host.
3. Shellport shows how many hosts it will run on before it starts. Production hosts need a tick every run.

Each host gets a pass/fail line with its output, and a summary such as "28 of 29 OK". The results are kept in Activity.

## Terminal guards

- **Multi-line pastes ask first.** Each pasted line could run as a command. By default Shellport asks on production hosts and in shells without safe paste; choose *Always* or *Never* in **Settings → Ask before pasting several lines**.
- **Notices from background terminals.** When a command that ran for 10 seconds or more finishes in a tab you're not looking at, or a background tab rings the bell, you get a notice. Turn these off in Settings.

## Clickable output

<kbd>Ctrl</kbd>+click an IP address, `host:port`, URL or archive name in terminal output for a small menu: copy it, open it in your browser (after confirming), or, for an archive, paste the command that extracts it.

## Activity and history

**Activity & history** in the sidebar is one local record of:

- database queries, searchable and re-run in one click;
- runs on many hosts, with each host's output;
- file transfers, server-to-server copies, and compress or extract jobs.

Filter by host or database, or search for SQL, commands and file names. *Activity: host* in the palette jumps straight to a host's history. Nothing in Activity contains passwords or other secrets. Encrypted backups include it only if you choose.

### Terminal logs

Tick **Save terminal output to a log file** in a host's *Advanced* settings to keep a plain-text log of each session, one file per session, with a timestamp on each line. Logs stay on this computer; the host editor shows the folder. Passwords you type aren't saved, because the terminal doesn't show them.

To save what's on screen once, use **Save terminal output…** in the command palette. It saves the terminal's scrollback to a file.
