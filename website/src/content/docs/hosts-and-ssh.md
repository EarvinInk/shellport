---
title: Hosts & SSH terminals
description: Save hosts in groups, choose how to sign in, and work in tabbed terminals.
section: Features
order: 10
---

## Terminals

- Tabbed **SSH** and **local** terminals with search, four colour themes, and one-click reconnect.
- **Hide the sidebar** with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>B</kbd> when you want the whole window for the terminal.
- Links in terminal output open in your browser only after you confirm, and only `http(s)` links.

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
