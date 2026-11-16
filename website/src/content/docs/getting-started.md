---
title: Quick start
description: Create your vault, add your servers and connect in a couple of minutes.
section: Getting started
order: 3
---

This guide takes you from a fresh install to a working SSH session, file transfer, Docker view and database query.

## 1. Create your vault

On first launch, open the **Vault** (shield icon on the left) and choose a master password. It encrypts everything you store and **cannot be recovered**, so keep it safe.

Tick *Unlock automatically via system keyring* if you don't want to type it after every login.

## 2. Add your servers

Either:

- click **+** in the Hosts panel and fill in name, address, user and how to sign in, or
- click the **import** icon next to it to bring in hosts from your `~/.ssh/config` (they stay in sync with it), or a CSV or JSON file.

Tick **Production server** on hosts where mistakes are costly. They get a PROD badge, and risky actions on them always ask first.

## 3. Connect

Double-click a host. The first time, Shellport shows the server's fingerprint. Check it and choose **Trust & connect**.

## 4. Transfer files

Hover a host and click the **folder** icon to open SFTP. Select a file or folder and use **Upload** / **Download**, or drag files in from your desktop.

## 5. Manage containers

Click the **cube** icon on the left, then pick **Local** or any SSH host.

## 6. Query a database

Click the **cylinder** icon on the left, then **+**. Enter the host, port, database and user, or paste a `postgres://` connection string. To reach a database that only listens on a server, pick that server under **SSH tunnel**. Open the connection, write SQL and press <kbd>Ctrl</kbd>+<kbd>Enter</kbd>. See [Databases](../databases/) for safety levels and transactions.

## 7. Forward a port

Hover a host, click the **⇄** icon and add a forward, e.g. local `8080` → `127.0.0.1:80` to open a server's internal web page in your browser.

## 8. Find anything fast

Press <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd> for the command palette: any host, database, open tab or action, most used first.

## 9. Make it yours

Theme, font, cursor, auto-lock time and more are under **Settings** (gear icon). From there you can also make an encrypted **backup** to move everything to another computer.
