# Changelog

All notable changes to Shellport are listed here. Versions follow [semantic versioning](https://semver.org).

## [Unreleased]

### Added
- Production marker for hosts and database connections: a PROD badge on rows, tabs and the palette, and risky SQL always asks first on a production connection (or one reached through a production host).
- Command palette (Ctrl+Shift+P) for hosts, databases, open tabs and app actions, ranked by use.
- Markdown notes per host; code blocks can be copied or pasted into the host's terminal without running.
- Up/down dots for hosts (a brief SSH greeting every few minutes while visible; can be turned off in Settings).
- Hosts imported from `~/.ssh/config` stay in sync with it both ways; Shellport edits only that host's HostName, User, Port, IdentityFile and ProxyJump lines and keeps a backup.
- Import and export hosts as CSV or JSON.
- Paste a `postgres://` connection string to fill in a database connection.
- Host and database address fields clean up pasted values (`user@host:port`, `ssh://…`, trailing `/`).
- Databases: a built-in PostgreSQL client with saved connections, a schema browser, a SQL editor (Ctrl+Enter runs the script or the selection), a result grid, query cancel, row limits and server notices.
- Database connections can go through any saved SSH host (including jump hosts) over a private tunnel; passwords are kept in the vault.
- Database support is driver-based, so more engines can be added without changing the rest of the app.
- Database safety levels per connection: *Confirm risky* asks before `UPDATE`/`DELETE` without `WHERE`, `DROP` and `TRUNCATE`; *Read-only* refuses writes. Connection colours mark their tabs.
- An open-transaction indicator with Commit and Rollback, and a warning before closing a tab or deleting a connection would roll it back.
- Database connections work behind PgBouncer and Supabase poolers (detected automatically, or set per connection).
- SSH terminals reconnect automatically after sleep, network changes or server restarts; each host can choose reconnect, ask or close.
- Optional tmux sessions per host keep programs running through dropped connections.
- Encrypted backup and restore of hosts, database connections, settings and (optionally) vault secrets, in Settings.
- Split panes: several terminals in one tab, side by side or stacked, restored on startup; save open terminals as a workspace and reopen it from the palette.
- Commands library: saved commands with `{{variables}}`, for all hosts or chosen hosts and groups, optionally run in new terminals on those hosts.
- Broadcast typing to every pane of a tab, with a banner while it is on and a confirmation when a production host is included.
- Run on hosts: run one command on many hosts and see a pass/fail line and output per host (`{{host}}`, `{{user}}` and `{{name}}` are filled in per host); production hosts need confirmation every run.
- Multi-line pastes into terminals ask first (on production hosts by default; always or never in Settings).
- A notice when a command that ran 10 seconds or more finishes in a background tab (the shell prompt is back), alongside the existing notices for the bell and for long output going quiet.
- Activity: one searchable record of queries (re-run in one click), multi-host runs and transfers, optionally included in backups; per-host terminal logs (opt-in) are plain text with a timestamp on each line, and *Save terminal output…* in the palette saves a terminal's scrollback to a file.
- One transfer queue for all file browser tabs: uploads, downloads and server-to-server copies with speed and time left, pause, resume and retry (continuing where each file stopped, also after a dropped connection), and a Transfers panel in the tab bar and palette.
- Favourite folders per host in the file browser, also listed in the palette.
- A built-in editor (CodeMirror) for remote text files, host notes and SQL: syntax highlighting, search, and a warning before saving over changes made on the server meanwhile. Saves never leave a half-written file, keep permissions, and write through symlinks.
- "Open in my editor" copies a remote file to a private temp folder, opens it in your editor (set the command in Settings) and uploads every save.
- Work as root in the file browser through sudo (the password is asked for each session and never stored); production hosts ask first.
- Compress files and folders on the server (.tar.gz or .zip) and extract archives into a new folder, as cancellable tasks recorded in Activity.
- trzsz (`trz` / `tsz`) in SSH terminals, and ZModem (`rz` / `sz`) where you turn it on for a host; files go through native file dialogs.
- Ctrl+click an address, URL or archive name in terminal output to copy it, open it, or fill in the command that extracts it.

### Changed
- Query results only fetch the rows you asked for from the server, and the grid draws only the visible rows, so large results no longer freeze the window.
- Each database tab keeps its own SQL draft.
- A dropped database connection reconnects without re-running the last query.
- Terminals batch output and slow the source down when drawing falls behind, so floods of output no longer freeze the app.

## [0.1.1] - 2026-11-10

### Added
- Terminal font (JetBrains Mono) is now bundled, so the terminal looks the same on every system.

### Changed
- The vault's keyring entry is tied to its config folder, so a second Shellport profile can't overwrite or remove it.
- Merged container logs open in time order.
- The SFTP "Modified" column no longer wraps.
- `.deb` package lists a proper maintainer, and the app ID is now `io.github.earvinink.shellport`.

## 0.1.0 - 2026-11-10

First public release (superseded by 0.1.1).

- Tabbed SSH and local terminals, saved hosts in groups, ProxyJump, port forwards, 2FA logins, import from `~/.ssh/config`.
- Dual-pane SFTP file manager with recursive transfers, drag and drop, cancel, and overwrite confirmation.
- Docker management for the local engine and over SSH: Compose projects, logs, shells, images, volumes, networks, cleanup.
- Encrypted vault (Argon2id + XChaCha20-Poly1305) for passwords, keys and tokens, with optional keyring unlock and ed25519 key generation.
- Strict host-key checking against `~/.ssh/known_hosts`.

[Unreleased]: https://github.com/EarvinInk/shellport/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/EarvinInk/shellport/releases/tag/v0.1.1

