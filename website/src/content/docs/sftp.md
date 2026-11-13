---
title: SFTP file manager
description: Browse, transfer, edit and manage files on your servers.
section: Features
order: 11
---

Hover a host and click the **folder** icon to open a dual-pane **local ↔ remote** browser. Double-click a folder to open it, or a file on the server to edit it.

## Transfers

All file browser tabs share **one transfer queue**. Open it with the **Transfers** button in the tab bar or from the command palette; each SFTP tab also lists its own host's transfers.

- Upload and download files and whole folders, or **drag and drop** files from your desktop file manager.
- Each row shows progress, **speed and time left**.
- **Pause** and **Resume**, or **Retry** after an error or a dropped connection. Transfers continue from where each file stopped instead of starting over; a file that changed at the source in the meantime is copied again from the start.
- **Cancel** removes the partly copied file. **Start now** moves a waiting transfer ahead of the queue.
- Up to four transfers run at once, at most two per host; the rest wait their turn.
- Shellport asks before overwriting local files, or files on another server.

Finished, failed and cancelled transfers are recorded in [Activity](../work-across-servers/#activity-and-history).

### Copy between servers

Select a file or folder and click **Copy to another host…**, then choose the host and folder. The data goes through Shellport directly from one server to the other, with no temporary copy on your disk. Copying to a production host asks first.

## Favourite folders

Click the **star** in the remote pane to save its folder for that host, and pick it from the **Favourites…** list later. Favourites also show up in the command palette as *host: /path*.

## Edit remote files

Double-click a text file, or select it and click **Edit**, to open it in a tab with syntax highlighting (shell, YAML, nginx, Dockerfile, JSON, TOML, Python, SQL, Markdown and more) and search.

- <kbd>Ctrl</kbd>+<kbd>S</kbd> saves. A dot on the tab marks unsaved changes, and closing the tab asks first.
- If the file **changed on the server** since you opened it, Shellport asks before replacing it: overwrite, reload (losing your changes) or save a copy on this computer.
- Saves never leave a half-written file: Shellport writes a temporary file next to it and swaps it in, keeping the file's permissions. Symlinks are saved through to the file they point to.
- Files up to 5 MB open in the editor. Binary files are refused; text that isn't UTF-8 opens read-only.

### Open in my editor

**Open in my editor** copies the file to a private temporary folder and opens it in your own editor. Every time you save there, Shellport uploads the file, with the same check for changes on the server. A strip in the SFTP tab lists the files being watched; **Stop** ends watching and removes the local copy, which is also removed when Shellport quits.

Set the editor in **Settings → Editor for "Open in my editor"**, for example `code --wait` or `gedit`. Leave it empty to use your desktop's default app.

## Work as root

Click the **shield** (*Work as root (sudo)*) in the remote pane to browse, transfer and edit as root; the pane title changes to **Root**. Shellport runs the server's SFTP program through `sudo`. If sudo needs a password, Shellport asks for it once per session and **never stores it**. On a production host, Shellport asks before switching to root.

The server needs `sudo` and its SFTP server program (installed with OpenSSH). *Open in my editor* isn't available while working as root.

## Compress and extract

- **Compress…** packs the selected file or folder on the server into a `.tar.gz` or `.zip` archive.
- **Extract here** unpacks an archive into a **new folder** named after it, so nothing is overwritten silently; if that folder exists, Shellport asks first. On production hosts, extracting always asks first.

Both run on the server (they need `tar`, or `zip` and `unzip`; Shellport says which is missing), can be cancelled, work as root when that's on, and are recorded in Activity.

## Transfers inside the terminal

In an SSH terminal, run **`trz`** to upload files to the server's current folder or **`tsz file…`** to download, and Shellport opens a file dialog. This uses [trzsz](https://trzsz.github.io/), which works through jump hosts, `su`, `sudo -i` and nested SSH sessions where SFTP can't reach. Install it on the server with `pip install trzsz` (or your distribution's package). Add `-b` for binary mode, which is about twice as fast. Press <kbd>Ctrl</kbd>+<kbd>C</kbd> to stop.

ZModem (`rz` / `sz` from lrzsz) also works where you turn on **Detect ZModem transfers** in the host's *Advanced* settings. It's off by default, because trzsz is faster and more reliable.

These transfers show their progress in the terminal, not in the transfer queue, and can't be paused. For big files, SFTP is much faster.

## Remote file operations

Create folders, rename, change permissions (**chmod**) and delete on the server.

## Current limits

- Uploads replace files of the same name on the server without asking (downloads and server-to-server copies ask first).
- Extracting always creates a new folder named after the archive, so an archive that already holds one top-level folder ends up one level deeper.
- ZModem hasn't yet been tested against lrzsz on a real server.

## Safety

- Downloads never write outside the folder you choose and never follow symlinks, even if a server sends hostile file names.
- In-terminal transfers can only read files you picked in a file dialog, and only write inside the folder you picked.
- File names are always passed to server commands (compress, extract, sudo) as quoted arguments, never as part of the command.
