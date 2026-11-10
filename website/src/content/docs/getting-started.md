---
title: Quick start
description: Create your vault, add your servers and connect in a couple of minutes.
section: Getting started
order: 3
---

This guide takes you from a fresh install to a working SSH session, file transfer and Docker view.

## 1. Create your vault

On first launch, open the **Vault** (shield icon on the left) and choose a master password. It encrypts everything you store and **cannot be recovered**, so keep it safe.

Tick *Unlock automatically via system keyring* if you don't want to type it after every login.

## 2. Add your servers

Either:

- click **+** in the Hosts panel and fill in name, address, user and how to sign in, or
- click the **import** icon next to it to bring in hosts from your `~/.ssh/config`.

## 3. Connect

Double-click a host. The first time, Shellport shows the server's fingerprint. Check it and choose **Trust & connect**.

## 4. Transfer files

Hover a host and click the **folder** icon to open SFTP. Select a file or folder and use **Upload** / **Download**, or drag files in from your desktop.

## 5. Manage containers

Click the **cube** icon on the left, then pick **Local** or any SSH host.

## 6. Forward a port

Hover a host, click the **⇄** icon and add a forward, e.g. local `15432` → `127.0.0.1:5432` to reach a remote database.

## 7. Make it yours

Theme, font, cursor, auto-lock time and more are under **Settings** (gear icon).
