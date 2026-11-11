<div align="center">

<img src="public/logo.svg" alt="Shellport logo" width="96" height="96">

# Shellport

**SSH, SFTP and Docker in one native Linux app.**

Terminals, file transfers, containers and an encrypted secrets vault, all in a single fast desktop window.

[![Latest release](https://img.shields.io/github/v/release/EarvinInk/shellport?color=8b5cf6&label=download)](https://github.com/EarvinInk/shellport/releases/latest)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux%20x86__64-8b5cf6)](#install)
[![License: MIT](https://img.shields.io/badge/license-MIT-8b5cf6)](LICENSE)

<img src="docs/screenshots/terminal.png" alt="Shellport showing saved hosts grouped by environment and an SSH session" width="900">

</div>

---

## Contents

- [What Shellport offers](#what-shellport-offers)
- [Install](#install)
- [Getting started](#getting-started)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Security and privacy](#security-and-privacy)
- [Current limits](#current-limits)
- [Uninstall](#uninstall)
- [Build from source](#build-from-source)
- [License](#license)

## What Shellport offers

### Terminals and saved hosts

- Tabbed **SSH** and **local** terminals with search and four colour themes.
- **Reconnects on its own** after sleep, Wi-Fi changes or server restarts (per host: reconnect, ask, or close the tab).
- Optional **tmux sessions** per host, so programs keep running through a dropped connection and reconnecting picks up where you left off.
- Stays responsive when a command floods the terminal with output.
- Save hosts in **groups** (Production, Staging, …) with colours and tags, and mark production servers: they get a **PROD** badge everywhere, and databases reached through them always confirm risky statements.
- **Command palette** (Ctrl+Shift+P): jump to any host, database, tab or action, most used first.
- **Notes per host** in Markdown; commands in them can be copied or pasted into the host's terminal (never run on their own).
- **Up/down dots** show which hosts are reachable (a brief check every few minutes; can be turned off).
- Sign in with a **password, a key in the vault, a key file, or ssh-agent**.
- **Two-factor (keyboard-interactive) logins**, e.g. one-time codes.
- **Jump hosts (ProxyJump)**: reach private servers through a bastion.
- **Port forwarding** (`ssh -L`) per host, with optional auto-start on connect.
- **Import from `~/.ssh/config`** (including `Include`, `Host *` defaults and `ProxyJump`), and keep those hosts **in sync both ways**: edits in the file show up in Shellport, and edits in Shellport change only that host's lines, keeping your comments and other settings.
- **Import and export hosts as CSV or JSON** (no secrets in the file).
- A **startup command** per host, e.g. `cd /srv/app`.
- **Hide the sidebar** (Ctrl+Shift+B) when you want the whole window for the terminal.

### Work across servers

- **Split panes**: several terminals in one tab, side by side or stacked, restored on startup. Save your open terminals as a **workspace** and reopen it from the palette.
- **Commands library**: saved commands with `{{variables}}` (e.g. `{{service:nginx}}`), for all hosts or chosen hosts and groups. Paste them (never pressing Enter for you), or run them in every new terminal on those hosts.
- **Broadcast typing** to every pane of a tab, with a banner while it's on.
- **Run on hosts**: one command on many hosts at once, with a pass/fail line and output per host ("28 of 29 OK"). `{{host}}`, `{{user}}` and `{{name}}` are filled in per host.
- Production hosts always ask before broadcasting or running a command on them.
- **Multi-line pastes ask first** (on production hosts by default; always or never in Settings), and a notice tells you when a long command finishes in a background tab.
- **Ctrl+click** an address, URL or archive name in terminal output to copy it, open it, or paste the command that extracts it.
- **Activity & history**: one searchable local record of database queries (re-run in one click), multi-host runs and transfers. Opt-in **terminal logs** per host (plain text, a timestamp on each line), and *Save terminal output…* for a terminal's scrollback.

### SFTP file manager

<img src="docs/screenshots/sftp.png" alt="Dual-pane SFTP file manager with local files on the left and the server on the right" width="900">

- **Dual-pane** local ↔ remote browser.
- **One transfer queue** for every tab: upload, download and **copy between servers**, with speed, time left, **pause, resume and retry** (a dropped connection continues where it stopped). Asks before overwriting local files or files on another server.
- **Edit remote files** in the built-in editor (with syntax highlighting), or **open them in your own editor** and every save is uploaded. If the file changed on the server meanwhile, Shellport asks before replacing it.
- **Work as root** with sudo (the password is asked for each session and never stored), and **compress or extract** archives on the server.
- **trzsz** (`trz` / `tsz`) in SSH terminals, and ZModem (`rz` / `sz`) where you turn it on for a host.
- **Favourite folders** per host, also in the command palette.
- **Drag and drop** files from your desktop file manager to upload.
- Create folders, rename, change permissions (chmod), and delete on the server.

### Docker, local or over SSH

<img src="docs/screenshots/docker.png" alt="Docker view listing the containers of a Compose project with ports and actions" width="900">

- Manage the **local Docker engine** or **any SSH host** (no Docker port needs to be exposed).
- Containers **grouped by Compose project**; start, stop, restart, pause, kill and remove, or open a shell inside a container.
- **Live logs** for one container or **all containers of a project, merged and colour-coded**:

<img src="docs/screenshots/logs.png" alt="Merged, colour-coded logs from all containers of a Compose project" width="900">

- **Images, volumes and networks**: pull and remove images (including from private registries); inspect and remove volumes and networks.
- **Disk usage and cleanup** (prune) in the System tab.

### Databases

- Built-in **PostgreSQL client**: saved connections, a lazy **schema browser** (schemas, tables, views, functions, columns with types and primary keys), a **SQL editor** and a **result grid**.
- Connect **directly or through any saved SSH host**. The tunnel is private to Shellport (an ephemeral port on 127.0.0.1) and honours jump hosts.
- Passwords live in the **vault**; TLS modes from *disable* to *verify-full*.
- Run whole scripts or just the selection (Ctrl+Enter), **cancel** long queries, cap rows per statement, see server notices, copy cells or results as TSV.
- Paste a **`postgres://` connection string** to fill in a connection; its password goes to the vault.
- **Safe on production**: per connection, choose *Confirm risky* (asks before `UPDATE`/`DELETE` without `WHERE`, `DROP`, `TRUNCATE`) or *Read-only*; the connection's colour marks its tabs.
- An **open-transaction** indicator with Commit / Rollback, and a warning before closing a tab would roll it back.
- Large results stay fast: only the requested rows leave the server, and the grid draws just what is on screen.
- Works behind **PgBouncer / Supabase poolers**, and reconnects after a dropped connection without re-running your query.
- More engines (MySQL, SQLite, …) can be added as drivers; see *Adding a database engine* below.

### Encrypted vault

<img src="docs/screenshots/vault.png" alt="The vault holding passwords and an ed25519 private key" width="900">

- Keep **passwords, private keys and API tokens** in one encrypted file, protected by a master password.
- **Generate ed25519 keys** and copy the public key for a server's `authorized_keys`.
- **Auto-lock** after a period of inactivity.
- Optionally **remember the vault** in your login keyring (GNOME Keyring / KWallet) so it unlocks when you log in.
- **Encrypted backup & restore** (Settings): hosts, database connections, settings and, if you choose, vault secrets in one passphrase-protected file, for moving to a new machine. No account or cloud involved.

### Safe by default

<img src="docs/screenshots/hostkey.png" alt="Unknown host key prompt showing the server's SHA256 fingerprint" width="900">

- Every server's identity is checked against `~/.ssh/known_hosts`. New servers show their fingerprint before anything is sent; a **changed key** is blocked with a clear warning.
- See [Security and privacy](#security-and-privacy) for details.

## Install

Shellport runs on 64-bit Linux (x86_64). It's developed and tested on Linux Mint 22, and should work on Ubuntu 22.04+, Debian 12+ and other current distributions that ship WebKitGTK 4.1.

Download the latest version from the **[Releases page](https://github.com/EarvinInk/shellport/releases/latest)**. There are two formats:

| File | Best for | Size |
|---|---|---|
| `Shellport_<version>_amd64.deb` | Ubuntu, Debian, Linux Mint, Pop!_OS | ~7 MB |
| `Shellport_<version>_amd64.AppImage` | Any distribution, no install needed | ~85 MB |

### Option A: .deb package (recommended on Ubuntu/Debian/Mint)

1. Download `Shellport_<version>_amd64.deb` from the [Releases page](https://github.com/EarvinInk/shellport/releases/latest).
2. Open a terminal in your Downloads folder and run:

   ```bash
   sudo apt install ./Shellport_*_amd64.deb
   ```

   apt installs the two system libraries Shellport needs (WebKitGTK and GTK) if they aren't already present.
3. Open **Shellport** from your applications menu, or run `shellport` in a terminal.

To update, download the newer `.deb` and run the same command.

### Option B: AppImage (any distribution)

1. Download `Shellport_<version>_amd64.AppImage` from the [Releases page](https://github.com/EarvinInk/shellport/releases/latest).
2. Make it executable and run it:

   ```bash
   chmod +x Shellport_*_amd64.AppImage
   ./Shellport_*_amd64.AppImage
   ```

   You can also right-click the file → **Properties** → **Permissions** → *Allow executing file as program*, then double-click it.

> **AppImage won't start?** AppImages need FUSE 2. Install it with
> `sudo apt install libfuse2t64` (Ubuntu 24.04+) or `sudo apt install libfuse2` (Ubuntu 22.04 / Debian).

## Getting started

1. **Create your vault.** On first launch, open the **Vault** (shield icon on the left) and choose a master password. It encrypts everything you store and **cannot be recovered**, so keep it safe. Tick *Unlock automatically via system keyring* if you don't want to type it after every login.
2. **Add your servers.** Either:
   - click **+** in the Hosts panel and fill in name, address, user and how to sign in, or
   - click the **import** icon next to it to bring in hosts from your `~/.ssh/config`.
3. **Connect.** Double-click a host. The first time, Shellport shows the server's fingerprint; check it and choose **Trust & connect**.
4. **Transfer files.** Hover a host and click the **folder** icon to open SFTP. Select a file or folder and use **Upload** / **Download**, or drag files in from your desktop.
5. **Manage containers.** Click the **cube** icon on the left, then pick **Local** or any SSH host.
6. **Open a database.** Click the **cylinder** icon on the left, then **+**. Enter the database host as the SSH server sees it (often `127.0.0.1`), pick the SSH host under *SSH tunnel*, and click **Test connection**. Double-click the saved entry to browse tables and run SQL.
7. **Forward a port.** Hover a host, click the **⇄** icon and add a forward, e.g. local `15432` → `127.0.0.1:5432` to reach a remote database.
8. **Make it yours.** Theme, font, cursor, auto-lock time and more are under **Settings** (gear icon).

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| Ctrl+Shift+T | New local terminal |
| Ctrl+Shift+W | Close tab |
| Ctrl+PgUp / Ctrl+PgDn | Previous / next tab |
| Ctrl+Shift+B | Show / hide the sidebar |
| Ctrl+Shift+P | Command palette |
| Ctrl+Shift+C / Ctrl+Shift+V | Copy / paste in the terminal |
| Ctrl+Shift+F | Search in the terminal or logs |
| Ctrl+S | Save (remote file editor) |
| Ctrl+Enter | Run the SQL script or selection (database tab) |
| Ctrl+click in terminal output | Copy or open an address, URL or archive name |
| Middle-click a tab | Close it |

## Security and privacy

- **No telemetry, no accounts, no cloud.** Shellport only connects to the servers and Docker engines you tell it to.
- **Vault encryption:** Argon2id (64 MiB, 3 passes) derives the key from your master password; contents are encrypted with XChaCha20-Poly1305, and any tampering with the file is detected.
- **Remembered vaults** store the vault key in your login keyring. Any program running as you can read it while the keyring is unlocked, so leave this off on shared machines.
- **Host keys** are verified against `~/.ssh/known_hosts` (hashed names, wildcards and `@revoked` entries included). On a changed key Shellport sends no password or key unless you explicitly replace the saved key.
- **Downloads** never write outside the folder you choose and never follow symlinks, even if a server sends hostile file names.
- **Links** in terminal output open in your browser only after you confirm, and only `http(s)` links.
- Your data lives in `~/.config/shellport/` (`hosts.json`, `settings.json`, `vault.bin`), readable only by you (mode 0600).

Found a security issue? Please report it privately via [GitHub security advisories](https://github.com/EarvinInk/shellport/security/advisories/new) rather than a public issue.

## Current limits

Files and transfers:

- The built-in editor opens text files up to 5 MB; larger or binary files can be downloaded or opened in your own editor (up to 200 MB).
- "Open in my editor" isn't available while working as root.
- trzsz and ZModem transfers show their progress in the terminal, not in the transfer queue, and can't be paused. They run at a few MB/s; SFTP is much faster for big files.
- ZModem (`rz` / `sz`) is off by default and hasn't yet been tested against lrzsz on a real server.
- Uploads replace files of the same name on the server without asking (downloads and server-to-server copies ask first).
- Extracting an archive always creates a new folder named after it, so an archive that already holds one top-level folder ends up one level deeper.

## Uninstall

```bash
sudo apt remove shellport
```

For the AppImage, just delete the file. Your hosts, settings and vault stay in `~/.config/shellport/`; delete that folder too if you want to remove everything (this permanently deletes your vault).

## Build from source

You need Rust (stable), Node.js 20+ with pnpm, and the WebKitGTK development packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

```bash
git clone https://github.com/EarvinInk/shellport.git
cd shellport
pnpm install
pnpm tauri dev                                   # run in development
pnpm tauri build --bundles deb,appimage          # → src-tauri/target/release/bundle/
```

Run the tests with `cd src-tauri && cargo test`. A full release build needs about 5 GB of free disk space.

Official downloads are built by GitHub Actions ([`.github/workflows/release.yml`](.github/workflows/release.yml)) on Ubuntu 22.04, so they run on that release and newer. Packages you build yourself only run on your distribution version or newer. To publish a release, bump the version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`, then push a matching tag (`git tag v0.2.0 && git push origin v0.2.0`). The workflow creates a draft release with both packages attached for you to review and publish.

<details>
<summary>Test servers and project layout</summary>

Disposable test targets (SSH on `127.0.0.1:2222`, user `test`, password `shellport-test` or key `dev/test_ed25519`):

```bash
ssh-keygen -t ed25519 -N "" -f dev/test_ed25519   # first time only
docker compose -f dev/docker-compose.test.yml up -d
cd src-tauri && cargo run --example smoke          # headless protocol smoke test
cd src-tauri && cargo test --lib -- --ignored postgres_live   # database driver against the test Postgres
```

The compose file also starts PgBouncer in transaction mode on `127.0.0.1:56432` (for `cargo test --lib -- --ignored pgbouncer_live`) and PostgreSQL with demo data (user, password and database `shellport` / `shellport-test` / `shellport`). It is published on `127.0.0.1:55432` for direct connections, and is reachable as host `postgres`, port `5432` through the SSH test host.

```
src-tauri/src/
  vault/          encrypted vault + keyring integration
  store/          hosts.json, databases.json, settings.json, ~/.ssh/config parser
  db/             database client: engine-neutral traits, registry, drivers/
  ssh/            connections, auth, host keys, ProxyJump, port forwards
  known_hosts.rs  known_hosts verification
  term.rs         local PTY / SSH shell / docker exec sessions
  sftp.rs         file browsing and transfers
  docker*.rs      local + remote Docker (socket tunnel or CLI fallback)
  commands.rs     the command surface the UI calls
src/              React UI (views/, components/, lib/ipc.ts)
dev/              test containers and icon generator
```

**Adding a database engine.** Drivers never see SSH: the session layer opens the tunnel and hands them a plain local address. To add one:
1. add a variant to `DbEngine` in `src-tauri/src/db/mod.rs`;
2. implement `DbDriver` and `DbSession` in `src-tauri/src/db/drivers/<engine>.rs` (`postgres.rs` is the reference);
3. map it in `src-tauri/src/db/registry.rs`;
4. add its entry (default port, TLS modes, extra fields) to `src/lib/dbEngines.ts`.

Profiles store engine-specific settings in `options`, so the store, tunnel, commands, tabs and result grid need no changes.

Built with [Tauri 2](https://tauri.app) (Rust) and React, using [russh](https://github.com/Eugeny/russh), [tokio-postgres](https://github.com/sfackler/rust-postgres), [bollard](https://github.com/fussybeaver/bollard) and [xterm.js](https://xtermjs.org).

</details>

## License

[MIT](LICENSE) © 2026 The Shellport authors
