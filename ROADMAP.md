# Shellport roadmap

This roadmap is built from public complaints and feature requests about other SSH and database clients: DBeaver, pgAdmin, TablePlus, Beekeeper Studio, DataGrip, MySQL Workbench, DbVisualizer, Termius, Tabby, electerm, MobaXterm, PuTTY and Remote Desktop Manager. Each item links to the discussion that motivated it. It also draws on 20 comparable open-source projects, including the other apps named ShellPort (see [Similar projects](#similar-projects)). Researched November 2026.

**Principles that shape the order**
- **Local-first and private.** No account, no subscription and no telemetry. Paywalled sync and cloud storage are among the most common Termius complaints, so anything shared between devices stays optional and self-hosted.
- **Light and fast.** Slowness and memory use are DBeaver's most common review complaints, and freezing on large output is a recurring Tabby and DbVisualizer report. Every feature must keep the app responsive.
- **Safe on production.** Shellport is used against real servers, so mistakes that can't be undone get guard rails before convenience features do.
- **Fix what is shipped before adding more.**

**Status:** ✅ marks items that are built, ◐ items that are partly built (see the [changelog](CHANGELOG.md), *Unreleased*).

**Size:** S is about a day, M a few days, L a week or more. Milestones are ordered so each builds on the ones before; items inside a milestone can ship separately.

## How the pieces fit

Shellport is **one workspace for the servers you look after**. Everything below hangs off a few shared building blocks instead of standing alone:

| Building block | What it is | Used by |
|---|---|---|
| **Hosts** | One record per server (and the local machine), wherever it came from: Shellport, `~/.ssh/config`, another client, a connection string. Database connections, forwards, containers, notes and monitoring all attach to a host. | Every milestone |
| **Production marker** | One flag on a host or database connection. It drives the colour stripe, extra confirmations, and safer defaults everywhere. It generalises what 1.1 did for databases. | Databases, sending to many hosts, paste guard, edit rows |
| **Palette** | Ctrl+Shift+P reaches every host, open session, saved command and action. Features add their actions here instead of inventing their own menus. | Every milestone |
| **Commands library** | Saved commands with variables. One library feeds the palette, host notes, sending to many hosts and command suggestions. | M2, M5 |
| **Activity & history** | One local, per-host record of what happened: queries run, multi-host results, transfers, and (opt-in) terminal logs. Long operations report their progress here and can be cancelled here. | M2, M3, M6 |
| **Transfer queue** | One queue for every way files move: SFTP, server to server, trzsz/ZModem. | M3 |
| **Editor** | One text editor component (CodeMirror 6) for remote files, SQL and notes. | M3, M6, M1 |
| **Connection pipeline** | One path every connection goes through: secret lookup, pre-connect hook, route (direct, jump host, SOCKS5, ProxyCommand), algorithms, authentication. Terminals, files, containers and database tunnels all use it. | M4 |
| **Tunnels** | One list of everything forwarded (`-L`, `-R`, `-D`, database tunnels), with live statistics, also reachable from the tray. | M4, M2 |

### App rules

Every feature follows these, and they decide how ideas from other projects were adapted:

1. **Hosts are the centre.** A feature attaches to a host (or the local machine) and shows up in that host's menu and in the palette.
2. **Production is one switch.** Risky actions on a Production host or connection always ask first: risky SQL, sending to many hosts, multi-line paste, editing rows.
3. **Keyboard first.** Everything is reachable from the palette; lists are searchable.
4. **Nothing runs on a server unless you start it.** Background work is limited to a TCP reachability check (status dots) and to reads while a panel is visible (monitor, tunnel stats). Shellport never reads shell history.
5. **Long work is visible and cancellable,** and reports in Activity: transfers, queries, multi-host runs, reconnects.
6. **Secrets stay in the vault or your password manager,** never in config files, logs, history, diagnostics or the screen in streamer mode. Copied passwords are wiped from the clipboard.
7. **Local first.** New data (notes, commands, workspaces, history settings) lives in the config folder, is included in the encrypted backup, and is never sent anywhere. Terminal logging is opt-in.
8. **The server can't reach into your machine silently.** Links, clipboard writes (OSC 52) and notifications from remote programs need approval or are off.
9. **Stays light on Linux.** Idle cost near zero; panels only work while visible; Linux is the first-class platform.
10. **Honest about limits.** The README's *Current limits* section is updated with every release.

---

## Shipped

### Phase 0: fix what is shipped (database client)


These affect the PostgreSQL client that is already built.

| # | Item | Why | Size |
|---|------|-----|------|
| 0.1 ✅ | **Work behind connection poolers.** Use unnamed statements (`query_typed`) instead of named prepared statements for queries and schema lookups. | Behind PgBouncer or Supabase's pooler in transaction mode, named prepared statements fail with `prepared statement "s0" does not exist` ([DataGrip thread][dg-pgb], [Supabase][supa-prep]). Our driver currently prepares every query this way. | S |
| 0.2 ✅ | **A separate draft per tab.** Save editor text per tab, not per database. | Two tabs on the same database currently overwrite each other's draft, and losing query text is a long-standing complaint ([Workbench #109360][wb-109360], [pgAdmin #366][pga-366]). | S |
| 0.3 ✅ | **Large results that don't freeze.** Draw only the visible rows, and stop reading from the server once the row limit is reached. | Big grids freezing the GUI is a top complaint ([DbVisualizer][dbvis-slow], [Workbench #67717][wb-67717]). We cap rows but still draw all of them and still download everything the server sends. | M |
| 0.4 ✅ | **Survive dropped connections.** Turn on TCP keep-alives for database connections, detect a lost session, and offer "Reconnect" (optionally automatic) while keeping the editor text. | Beekeeper tracks SSH tunnels that drop and don't reconnect as a bug ([#2165][bk-issues]); Metabase had to fix tunnel reconnects ([#6114][mb-6114]). | M |

### Phase 1: safety and reliability

| # | Area | Item | Why | Size |
|---|------|------|-----|------|
| 1.1 ✅ | DB | **Production safety.** A read-only option per connection, enforced by the server (`default_transaction_read_only`). A confirmation before `UPDATE`/`DELETE` without `WHERE`, `DROP` or `TRUNCATE`. The connection colour shown as a stripe on its tab and editor. | Accidental `UPDATE` without `WHERE` is a classic ([MySQL safe updates][mysql-1175], [SQLAuthority][sqla]). TablePlus asks before running queries in [safe mode][tp-safe], and its users want it per connection tag ([TablePlus #2669][tp-2669]). | M |
| 1.2 ✅ | DB | **Open-transaction indicator.** Show "Transaction open" with Commit and Rollback buttons, and warn when closing a tab or disconnecting with an open transaction. | A forgotten `BEGIN` in a GUI holds locks and blocks cleanup on the server ([pganalyze][pga-idle], [Netdata][nd-idle]). DataGrip users asked for exactly this marker ([thread][dg-tx]). | M |
| 1.3 ✅ | SSH | **Automatic reconnect.** Reconnect after sleep or a network change, with backoff and a visible "reconnecting" state. A per-host choice between reconnect, ask and close tab. An option to run `tmux new -A -s shellport` on connect so work survives the drop. | Plain SSH dies when the IP changes or the laptop sleeps ([OpenReplay][or-unstable], [tmux guide][tmux]). Tabby users want reconnect behaviour configurable ([#8062][tabby-8062]); Warp users ask for it too ([#442][warp-442]). Today Shellport only has a manual Reconnect button. | M |
| 1.4 ✅ | SSH | **Responsive with heavy output.** Batch terminal writes and cap how often the screen redraws, so a runaway `cat` or a log flood never freezes the window. Test with a 100 MB stream. | Tabby reports [unresponsive tabs under fast output][tabby-slow]. | S |
| 1.5 ✅ | App | **Encrypted backup and restore.** Export hosts, database profiles, settings and (optionally) the vault to one encrypted file, and import it on another machine. | People leave Termius over paid sync and privacy ([Lemmy][lemmy-termius], [G2][g2-termius], [Terminator][term-sync]). An encrypted file the user controls covers moving machines without an account. | M |

---

## M1: Hosts at the centre

The foundation for everything after it: one host record, filled from wherever your servers already live, reachable from one palette.

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M1.1 ✅ | **One host record everywhere, with a Production marker.** Database connections, forwards, containers and notes link to a host; a Production flag on hosts and database connections sets the colour stripe, defaults new database connections to *Confirm risky*, and turns on the confirmations listed in the app rules. | 1.1 | Generalises the database safety work so terminals and multi-host actions get the same protection ([TablePlus #2669][tp-2669] asks for safety per tag). | M |
| M1.2 ◐ | **Hosts from where they already live:** read and edit `~/.ssh/config` faithfully (keeping comments, indentation, `Include`s and unknown settings) or use it as a read-only source; honour more of its keywords (trzsz-ssh's list); import from Termius, MobaXterm, PuTTY/KiTTY, SecureCRT, Xshell and CSV/JSON (export too); paste a `postgres://…` connection string to create a database connection, its password going to the vault. Every address field trims spaces and stray `/` and warns about obvious mistakes. | M1.1 | **Done:** two-way `~/.ssh/config` sync, CSV/JSON, connection strings, address cleanup. **Left:** importers for Termius, MobaXterm, PuTTY/KiTTY, SecureCRT and Xshell, and honouring more ssh_config keywords. Linux users keep hosts in that file ([purple][purple], [trzsz-ssh][tssh]); imports remove the switching cost ([Voltius][voltius], [nyaterm][nyaterm], [Termix][termix]); connection strings ([Antares #193][antares-193], [sqlectron #716][sqlectron-716], [Azure][azure-conn]); a trailing `/` broke DBeaver tunnels ([#1156][dbeaver-1156]). | L |
| M1.3 ✅ | **Palette (Ctrl+Shift+P)** for hosts, open sessions, saved commands and actions, ranked by use. Every later feature registers its actions here. (Not Ctrl+K: in a terminal that deletes to the end of the line.) | M1.1 | [omnyssh][omnyssh], [purple][purple], [opsdeck][opsdeck], WindTerm and nyaterm. | M |
| M1.4 ✅ | **Notes per host** in Markdown (a plain editor for now; moves to the shared editor with M3.2), with commands you can copy or send to the terminal; included in backups. | M1.1 | [Termix][termix] and [opsdeck][opsdeck]. | S |
| M1.5 ✅ | **Up/down dots** for hosts, from a short SSH greeting every few minutes (no login, like `ssh-keyscan`), only while the window is visible; can be turned off in Settings in case intrusion-prevention tools object. | M1.1 | See which servers are up before you click ([siqi2000][sq-sp], [Termix][termix], [purple][purple]). | S |

## M2: Work across servers

Several terminals at once, the same command on many servers, and a record of what happened.

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M2.1 | **Split panes and workspaces.** Several terminals in one tab, side by side or stacked; save a layout of hosts as a workspace and reopen it from the palette. The existing *Restore tabs on startup* setting extends to panes and layouts; with tmux (1.3) work picks up exactly where it was. | M1.3 | [AlternativeTo][alt-moba]; [WindTerm][windterm], [Termix][termix], [Voltius][voltius] and [nyaterm][nyaterm] restore workspaces. | L |
| M2.2 | **Commands library:** saved commands with variables (e.g. `{{service}}`), global or per host or group, optionally run on connect. Used from the palette, host notes, M2.3 and M5.3. | M1.3 | [electerm issues][electerm-issues]; RDM runs snippets on connect ([forum][rdm-broadcast]). | M |
| M2.3 | **Send to many hosts,** two modes sharing one target picker: *live* (type once into chosen panes, with a banner while on) and *run and collect* (a command from the library, how many hosts it will touch shown first, then a pass/fail line and output per host, e.g. "28 of 29 OK"). Production hosts in the set need confirmation. Results go to Activity. | M2.1, M2.2, M1.1 | [RDM 1][rdm-bc1], [RDM 2][rdm-bc2], [i2cssh][i2cssh]; [purple][purple], [omnyssh][omnyssh], [Termix][termix]. | M |
| M2.4 | **Terminal guards:** confirm multi-line pastes (always on Production hosts, optional elsewhere), and an optional notice when a long command finishes in a background tab. | M1.1 | [Tabby][tabby]. | S |
| M2.5 | **Activity & history,** one local per-host record: database query history (searchable, re-run in one click), multi-host results, transfers, and opt-in terminal logs (plain text with timestamps; save output or scrollback on demand). Never stores secrets; included in backups only if you choose. | M1.1 | [pgAdmin][pga-4v3]; [Tabby save-output][tabby-save], [Termius][termius-logs], [RDM][rdm-log]. | M |
| M2.6 | **Clickable output:** Ctrl+click an IP, `host:port`, URL or archive name to copy it, open it (after confirming, as links already do), fill in the extract command, or open a tunnel to that port (with M4.1). | M1.3, M4.1 | [opsdeck][opsdeck], [Netcatty][netcatty], nyaterm. | M |

## M3: Files

Every way of moving and editing files, behind one queue and one editor.

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M3.1 | **Transfer queue** for SFTP, server-to-server copies and in-terminal trzsz/ZModem (`trz`/`tsz`, `rz`/`sz`, which reach through jump hosts and nested shells): pause, resume and retry, speed and time left, favourite paths per host, history in Activity. | M2.5 | Ours can only cancel today ([conghuahuadan][cg-sp]); [Termix][termix] and [Voltius][voltius] copy server to server; [trzsz-ssh][tssh], [electerm][electerm], [WindTerm][windterm]. | L |
| M3.2 | **One editor** (CodeMirror 6) for remote files, notes and SQL. Remote files: edit and save back with a warning if the file changed on the server meanwhile, a size limit for plain text, and a "save a copy" fallback; or **Open in my editor**, which uploads on each save. | — | [conghuahuadan][cg-sp]; [nyaterm][nyaterm] and [XPipe][xpipe] open files in your own editor. | M |
| M3.3 | **Server-side file actions:** use sudo to read and edit root-owned files (asks every session, never stored), compress and extract on the server. | M3.1 | [XPipe][xpipe] (sudo), [Netcatty][netcatty] (archives). | M |

## M4: Connections

Everything about how Shellport reaches and authenticates to a server, in one pipeline, so terminals, files, containers and database tunnels all gain each feature at once.

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M4.1 | **Tunnels view:** local (`-L`), remote (`-R`) and dynamic SOCKS5 (`-D`) forwards plus database tunnels in one list, with throughput, uptime and open connections while visible; start and stop from the palette and the tray. | M1.3 | [electerm #3423][electerm-3423], [PuTTY][putty-dyn]; [purple][purple] and [rssh][rssh] show tunnel stats. | M |
| M4.2 | **Routes and pre-connect hooks:** each host connects directly, through a jump host (as today), a SOCKS5 proxy, or a ProxyCommand from `~/.ssh/config`. An optional local hook runs first (e.g. "is the VPN up?"); a non-zero exit stops the connection and its output is shown. No shell interpolation; host details passed as environment variables. | M1.2 | [Snuffy2/shellport][sn-sp] ([config][sn-config]). | M |
| M4.3 | **Authentication:** OpenSSH certificates for users and hosts (`@cert-authority`); hardware security keys (FIDO2, `sk-ed25519`), via ssh-agent first; agent forwarding per host, off by default with a trust warning; one-click key setup (generate in the vault, add to `authorized_keys`, switch the host to key login, offer to fix local key file permissions; never edits `sshd_config`). | M1.1 | Teams with an SSH CA; [PuTTY][putty-fido], [Termius][termius-fido]; [Tabby][tabby-agent]; [omnyssh][omnyssh], [purple][purple], [opsdeck][opsdeck]. | L |
| M4.4 | **Secret sources:** besides the vault, take a password or key passphrase at connect time from the system keyring, `pass`, KeePassXC, Bitwarden or 1Password CLI. The same picker in host and database editors. Copied secrets are wiped from the clipboard after 30 seconds. | M1.1 | [XPipe][xpipe], [purple][purple], [opsdeck][opsdeck] ("wiped after 30 s"). | M |
| M4.5 | **Per-host protocol options:** algorithm presets (Compatible, Secure, Custom, with risk labels) for old routers and appliances; X11 forwarding using the local `$DISPLAY`. | M1.1 | [nyaterm][nyaterm]; [r/selfhosted][x11-reddit], [Tabby][tabby-x11]. | M |

## M5: See your servers

What is happening on a server, without leaving Shellport and without installing anything on it.

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M5.1 | **System monitor panel** beside a terminal: CPU, memory, disk and network, with processes and connections on demand, read over the existing connection only while the panel is open. | M1.1 | [conghuahuadan][cg-sp] and five more of the projects reviewed. | L |
| M5.2 | **Podman** alongside Docker in the containers view, locally and over SSH. | — | [XPipe][xpipe], [purple][purple], [Termix][termix]. | S–M |
| M5.3 | **Command suggestions** (opt-in per host): while typing a command name, suggest commands found on the server's `PATH` (read once per connection) and from the commands library, with a one-line offline description (bundled tldr-pages). Tab or Enter only fills the name. Never reads shell history. | M2.2 | The headline feature of [conghuahuadan][cg-sp]. | L |

## M6: Databases, deeper

The database client grows on the shared editor, history and Production marker.

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M6.1 | **SQL editor on the shared editor:** syntax highlighting; autocomplete for tables and columns that follows aliases and the tables in the query, from the schema already loaded; word-wrap toggle (Alt+Z). Query history comes from M2.5. | M3.2, M2.5 | [sqltools #110][sqltools-110], [Workbench #52917][wb-52917], [CodeMirror][cm-sql], [r/SQL][bk-reddit]. | L |
| M6.2 | **Copy and export results** as CSV, JSON or INSERT statements, selected rows or all, to the clipboard or a file. | — | [r/SQL][bk-reddit]. | S |
| M6.3 | **Show a table's CREATE statement** from the right-click menu. | — | [r/SQL][bk-reddit]. | S |
| M6.4 | **EXPLAIN view:** a readable plan tree with costs and the slowest steps highlighted. | — | [Devart][devart]. | L |
| M6.5 | **Edit rows in the grid:** pending changes, a preview of the generated SQL, then Save or Discard; needs a primary key; always confirmed on Production connections, refused on Read-only. | 1.1, 1.2, M1.1 | [TablePlus][vibe]. | L |
| M6.6 | **More engines:** MySQL/MariaDB, then SQLite (local, or over SFTP). | — | The driver design already supports this. | L each |

## M7: Bigger bets

| # | Item | Builds on | Why | Size |
|---|------|-----------|-----|------|
| M7.1 | **Mosh or Eternal Terminal** as a per-host transport for unreliable networks, using the same reconnect UI as 1.3. | M4.2 | [How-To Geek][mosh]; [Snuffy2/shellport][sn-sp] ships both. | L |
| M7.2 | **Serial console** as another tab type, next to local terminals. | M2.1 | Tabby, MobaXterm, WindTerm, electerm. | M |
| M7.3 | **Optional self-hosted sync** of the encrypted backup (1.5): a file in a folder you already sync, or your own WebDAV server. Never a Shellport-hosted service. | 1.5 | [Lemmy][lemmy-termius], [Terminator][term-sync], [conghuahuadan][cg-sp]. | L |

## Platform and trust (ongoing)

Small items that ship alongside any milestone.

| # | Item | Why | Size |
|---|------|-----|------|
| PL.1 | **Linux packages:** signed apt and rpm repositories, AUR, Flatpak, a Nix flake and arm64 builds. Update notices point to the package manager; only the AppImage offers an in-app update, verified with SHA-256 and opt-in. | [Voltius][voltius], [electerm][electerm], [Netcatty][netcatty]; [conghuahuadan][cg-sp] verifies updates. | M |
| PL.2 | **Redaction, used twice:** streamer mode masks IP addresses, hostnames and usernames on screen; diagnostic export uses the same redaction for bug reports. | [omnyssh][omnyssh], [WindTerm][windterm]; [conghuahuadan][cg-sp]. | S |
| PL.3 | **Light theme** for the whole app, following the system setting. | [conghuahuadan][cg-sp]. | M |
| PL.4 | **Tray and window:** close to tray (keeping tunnels and forwards running, listed in the tray menu), remember window position and size, and an optional drop-down terminal on a global hotkey (via the desktop portal on Wayland, where available). | [conghuahuadan][cg-sp]; [Tabby][tabby], [electerm][electerm]. | M |
| PL.5 | **Terminal hardening:** cap oversized escape sequences (OSC/DCS) and check xterm.js's limits; OSC 52 clipboard writes stay off, or ask (app rule 8). | [igorls/shellport][ig-sp]. | S |
| PL.6 | **Test hook in development builds:** read the terminal grid and send input from tests. Never in release builds. | [igorls/shellport][ig-sp]. | S |
| PL.7 | **Publish memory and startup numbers** against other clients for each release (app rule 9). | [omnyssh][omnyssh], [Voltius][voltius]. | S |
| PL.8 | **Current limits** section in the README and docs, kept up to date (app rule 10). | [conghuahuadan][cg-sp], [igorls][ig-sp]. | S |

## Similar projects

Four other projects are named ShellPort. What we can learn from each:

| Project | What it is | Learn from it |
|---|---|---|
| [conghuahuadan/ShellPort][cg-sp] | Desktop SSH + SFTP client for macOS and Windows (closest to us). | Split panes, command suggestions, system monitor, richer transfers, remote editor, WebDAV sync, updater, honest limits (M2.1, M5.3, M3.1, M3.2, PL.8, M5.1, M7.3, PL.1–PL.4). It lacks jump hosts, agent, hardware keys, 2FA logins, auto-reconnect and Linux packages, and trusts host keys without showing them. |
| [Snuffy2/shellport][sn-sp] | Browser-based SSH/Telnet/Mosh/ET client (fork of sshwifty). | Mosh/ET, pre-connect hooks, SOCKS5, secrets from files or environment variables (M7.1, M4.2). |
| [igorls/shellport][ig-sp] | Share your own shell in a browser; one binary, end-to-end encryption, TOTP. | Terminal hardening, test hook, security-model documentation (PL.5, PL.6, PL.8). |
| [siqi2000/ShellPort][sq-sp] | Reach a home terminal from a phone through a relay server. | Host reachability dots (M1.5). |

Other comparable open-source clients reviewed (stars as of November 2026), and what each contributes above:

| Project | What it is | Learn from it |
|---|---|---|
| [Tabby][tabby] (75k) | Electron terminal and SSH client with plugins | Paste guard, finished-command notice, drop-down terminal (M2.4, PL.4) |
| [WindTerm][windterm] (32.6k) | Fast SSH/Telnet/serial client, partly open source | Workspace restore, ZModem, on-screen credential protection (M2.1, M3.1, PL.2) |
| [Termix][termix] (15.9k) | Self-hosted server manager, plugin-based | Workspaces, imports, notes, server-to-server SFTP; avoid its telemetry plugin (M2.1, M1.2, M1.4, M3.1) |
| [electerm][electerm] (15.6k) | Multi-protocol terminal and SFTP client | trzsz/ZModem, Linux packaging; avoid vendor cloud sync (M3.1, PL.1) |
| [XPipe][xpipe] (14.6k) | Connection hub driving local CLIs | Password-manager secrets, sudo in the file browser, open in your editor (M4.4, M3.3, M3.2) |
| [Netcatty][netcatty] (7.2k) | SSH workspace with SFTP | Clickable archive names, compress/extract on the server (M2.6, M3.3) |
| [trzsz-ssh][tssh] (2.8k) | Drop-in OpenSSH client | trzsz transfers, ssh_config keyword coverage (M1.2, M3.1) |
| [nyaterm][nyaterm] (2.3k) | Native remote workspace | Algorithm presets, imports, open in your editor (M4.5, M1.2, M3.2) |
| [omnyssh][omnyssh] (1.3k) | Rust dashboard, terminal and SFTP | Streamer mode, key setup, palette (PL.2, M4.3, M1.3) |
| [Voltius][voltius] (0.8k) | Tauri SSH client with self-hosted sync | One-click imports, packaging, memory comparison (M1.2, PL.1, PL.7) |
| [purple][purple] (0.8k) | Rust TUI that edits `~/.ssh/config` | Faithful config editing, multi-host runs, tunnel stats (M1.2, M2.3, M4.1) |
| [rssh][rssh] (0.5k) | Multi-platform SSH client | Tunnel statistics (M4.1) |
| [opsdeck][opsdeck] (0.2k) | Tauri ops workstation | Clipboard wipe, key permission check, clickable ports, notes (M4.4, M4.3, M2.6, M1.4) |

Also reviewed: Nexterm (server-based, little that transfers), Shell360 and OpsMaxx (confirm items already planned).

**Open decision: the name.** "ShellPort" is used by four other projects, including a desktop SSH client in the same category that started publishing on 2026-11-05, and the `shellport` names on npm and ghcr.io are taken. Decide whether to keep or change the name before investing further in the website, packages and app ID.

## Already strengths: keep them

- **Light, native app** (Tauri). Heaviness is the most common complaint about DBeaver ([G2][g2-dbeaver], [DbVis][dbvis-alt]) and Tabby ([review][tabby-heavy]).
- **One double-click to a terminal, files or a database.** pgAdmin users disliked clicking through a server tree every time ([thread][pga-clicks]).
- **SSH tunnels run inside the app**, so they can't outlive it (compare TablePlus's [post on leaked tunnels][tp-leak]). Tunnel reachability is checked before connecting, with plain-language errors.
- **Free, open source and local:** no account or subscription, which is what people leaving Termius are looking for.
- **Host key verification, encrypted vault and 2FA logins** are already built in. Tabby only [added host key checking][tabby-hostkey] recently.

## Not planned

- Accounts, hosted cloud sync or anything paid. See the principles above.
- Usage reporting of any kind, even anonymous (Termix ships a telemetry plugin; we won't).
- An AI assistant in the terminal. Not requested in the sources reviewed, and at odds with "private by default".

## Where the earlier items went

The roadmap was reorganised around the building blocks above. **No item was dropped.** Each one was kept, merged with a related item, or adapted to the app rules:

| Earlier | Now | What changed and why |
|---|---|---|
| 2.1 Query history | M2.5 | Merged into one Activity & history, shared with terminal logs, multi-host results and transfers. |
| 2.2 SQL editor | M6.1 + M3.2 | The CodeMirror base became the shared editor for files, notes and SQL; the SQL features stay in M6.1. |
| 2.3 Paste a connection string | M1.2 | One of several ways hosts and connections arrive. |
| 2.4 Copy and export results | M6.2 | Unchanged. |
| 2.5 Snippets | M2.2 | Now the commands library, used by the palette, notes, sending to many hosts and suggestions; its own palette folded into M1.3. |
| 2.6 Split panes | M2.1 | Combined with workspace restore. |
| 2.7 Broadcast input | M2.3 | Merged with multi-host runs as the *live* mode; Production hosts ask first (rule 2). |
| 2.8 Session logging | M2.5 | Part of Activity & history; opt-in and never stores secrets (rules 6, 7). |
| 2.9 Command suggestions | M5.3 | Adapted: opt-in per host, candidates from the server's `PATH` and the commands library (rule 4). |
| 2.10 Better SFTP transfers | M3.1 | Part of the one transfer queue. |
| 2.11 Edit remote files | M3.2 | Built on the shared editor; "Open in my editor" kept. |
| 2.12 Current limits | PL.8 + rule 10 | Became an app rule, kept up to date every release. |
| 2.13 Restore workspace | M2.1 | Extends the existing *Restore tabs on startup* setting instead of adding a second one. |
| 2.14 Import from other clients | M1.2 | One of the host sources. |
| 2.15 Command palette | M1.3 | Moved to the foundation; every feature registers its actions there. |
| 2.16 Run on many hosts | M2.3 | Merged with broadcast as the *run and collect* mode; results go to Activity. |
| 2.17 One-click key setup | M4.3 | Part of authentication; keys are created in the vault. |
| 2.18 Clickable output | M2.6 | "Open a tunnel to this port" uses the Tunnels view; links still confirm first (rule 8). |
| 2.19 Notes per host | M1.4 | Uses the shared editor; included in backups (rule 7). |
| 3.1 Dynamic forwarding | M4.1 | Part of the one Tunnels view. |
| 3.2 Remote forwarding | M4.1 | Part of the one Tunnels view. |
| 3.3 Agent forwarding | M4.3 | Part of authentication. |
| 3.4 X11 forwarding | M4.5 | Grouped with other per-host protocol options. |
| 3.5 OpenSSH certificates | M4.3 | Part of authentication. |
| 3.6 Hardware security keys | M4.3 | Part of authentication. |
| 3.7 CREATE statement | M6.3 | Unchanged. |
| 3.8 Catch host typos | M1.2 | Applied to every address field, not only databases. |
| 3.9 System monitor | M5.1 | Adapted: reads only while the panel is open (rules 4, 9). |
| 3.10 Up/down dots | M1.5 | Moved to the foundation; paused when the window is hidden (rule 9). |
| 3.11 Pre-connect hooks | M4.2 | Part of the connection pipeline, so they also cover database tunnels. |
| 3.12 Outbound SOCKS5 | M4.2 | One of the routes, alongside jump hosts and ProxyCommand. |
| 3.13 Faithful `~/.ssh/config` | M1.2 + M4.2 | Editing and import are host sources; ProxyCommand became a route. |
| 3.14 Password managers | M4.4 | Part of secret sources, with the same picker for hosts and databases. |
| 3.15 trzsz / ZModem | M3.1 | Shows in the same transfer queue. |
| 3.16 Tunnel monitor | M4.1 | Part of the Tunnels view; statistics only while visible. |
| 3.17 Algorithm presets | M4.5 | Per-host protocol options. |
| 3.18 Stronger SFTP | M3.1 + M3.3 | Server-to-server copies in the queue; sudo and archives as server-side actions. Sudo asks every session and is never stored (rule 6). |
| 3.19 Podman | M5.2 | Unchanged. |
| 4.1 More engines | M6.6 | Unchanged. |
| 4.2 EXPLAIN view | M6.4 | Unchanged. |
| 4.3 Edit rows | M6.5 | Adds Production confirmation and refusal on Read-only (rule 2). |
| 4.4 Mosh / Eternal Terminal | M7.1 | A transport choice in the connection pipeline; reuses the reconnect UI. |
| 4.5 Self-hosted sync | M7.3 | Unchanged; never a hosted service. |
| 4.6 Serial console | M7.2 | A tab type next to local terminals. |
| P.1 OSC 52 approval | rule 8 + PL.5 | Became an app rule covering anything a server can trigger on your machine. |
| P.2 Limit escape sequences | PL.5 | Unchanged. |
| P.3 In-app update | PL.1 | Adapted for Linux: package-manager notices first; in-app update only for the AppImage, opt-in. |
| P.4 Diagnostic log | PL.2 | Shares one redaction with streamer mode, so neither can leak what the other hides. |
| P.5 Light theme | PL.3 | Unchanged. |
| P.6 Close to tray | PL.4 | The tray also lists running tunnels. |
| P.7 Test hook | PL.6 | Unchanged. |
| P.8 Paste and clipboard guards | M2.4 + M4.4 | Split: paste guard and finished-command notice in terminals; clipboard wipe with secrets. |
| P.9 Streamer mode | PL.2 | Shares redaction with diagnostics. |
| P.10 Linux packages | PL.1 | Combined with update notices. |
| P.11 Drop-down terminal | PL.4 | Grouped with tray and window; on Wayland it depends on the desktop's global-shortcut portal, which is noted as a limit. |
| P.12 Memory comparison | PL.7 + rule 9 | Published each release. |

New in this reorganisation: the **Production marker on hosts** (M1.1) and the **one host record** linking everything. They are the glue that lets terminals, files and databases share safety and context; both extend what 1.1 already shipped.

---

[dg-pgb]: https://intellij-support.jetbrains.com/hc/en-us/community/posts/115000617430/comments/115000508724
[supa-prep]: https://supabase.com/docs/guides/troubleshooting/disabling-prepared-statements-qL8lEL.md
[wb-109360]: https://bugs.mysql.com/bug.php?id=109360
[pga-366]: https://postgrespro.com/list/thread-id/1097548
[dbvis-slow]: https://support.dbvis.com/support/discussions/topics/1000077515/page/3?sort=date
[wb-67717]: https://bugs.mysql.com/bug.php?id=67717
[bk-issues]: https://redirect.github.com/beekeeper-studio/beekeeper-studio/issues
[mb-6114]: https://github.com/metabase/metabase/issues/6114
[mysql-1175]: https://www.bytebase.com/reference/mysql/error/1175-using-safe-update-mode
[sqla]: https://blog.sqlauthority.com/?p=203508
[tp-safe]: https://docs.tableplus.com/gui-tools/code-review-and-safemode/safe-mode
[tp-2669]: https://github.com/TablePlus/TablePlus/issues/2669
[pga-idle]: https://pganalyze.com/docs/checks/connections/idle_transaction
[nd-idle]: https://www.netdata.cloud/guides/postgres/postgres-idle-in-transaction/
[dg-tx]: https://intellij-support.jetbrains.com/hc/en-us/community/posts/360008438320/comments/360001745299
[or-unstable]: https://blog.openreplay.com/3-ssh-alternatives-unstable-connections/
[tmux]: https://dev.to/idoko/persistent-ssh-sessions-with-tmux-25dm/comments
[tabby-8062]: https://www.goodfirstissue.org/Eugeny/tabby/issues/8062
[warp-442]: https://github.com/warpdotdev/warp/discussions/442
[tabby-slow]: https://lightrun.com/answers/eugeny-tabby-extremely-slow-output-causing-unresponsive-tab
[lemmy-termius]: https://deddit.petersanchez.com/g/selfhosted@lemmy.world/p/6Vp275vZdgjkC6fdYd-SSH-Client-for-Linux-Desktop-and-Android--Altern
[g2-termius]: https://www.g2.com/products/termius
[term-sync]: https://reddit.sentinel-team.org/posts/1th0rai/snapshots/2026-05-19T23%3A02%3A45.569672Z
[pga-4v3]: https://postgrespro.com/list/id/fefda762-bbe4-7fe7-76f9-4a44b3c99a61@intellasoft.net
[sqltools-110]: https://oss.issuehunt.io/r/mtxr/vscode-sqltools/issues/110
[wb-52917]: https://bugs.mysql.com/bug.php?id=52917
[cm-sql]: https://discuss.codemirror.net/t/sql-table-column-autocompletion-based-on-context/7232
[bk-reddit]: https://redlib.hbubli.cc/r/SQL/comments/1ctsmtr/what_feature_should_i_add_next_to_beekeeper_studio
[antares-193]: https://github.com/Fabio286/antares/issues/193
[sqlectron-716]: https://www.github.com/sqlectron/sqlectron-gui/issues/716
[azure-conn]: https://learn.microsoft.com/en-gb/azure/postgresql/development/vs-code-extension/advanced-connection-options
[electerm-issues]: https://www.Github.com/electerm/electerm/issues
[rdm-broadcast]: https://docs.devolutions.net/rdm/knowledge-base/how-to-articles/broadcast-actions-in-ssh-sessions.md
[rdm-bc1]: https://forum.devolutions.net/topics/37648/solved-feature-request-typ-in-multiple-windows-at-once
[rdm-bc2]: https://forum.devolutions.net/topics/31061/multiple-input-especially-for-ssh
[i2cssh]: https://github.com/wouterdebie/i2cssh
[alt-moba]: https://alternativeto.net/software/mobaxterm/?platform=linux
[tabby-save]: https://awesome.ecosyste.ms/projects/github.com%2FEugeny%2Ftabby-save-output
[termius-logs]: https://docs.termius.com/organize-and-connect-to-hosts/session-logs
[rdm-log]: https://forum.devolutions.net/topics/52005/provide-a-button-to-toggleenable-logging-on-demand
[electerm-3423]: https://git.tdem.in/electerm/electerm/issues/3423
[putty-dyn]: https://putty.cs.utah.edu/wishlist/portfwd-dynamic-remote.html
[tabby-agent]: https://www.github.com/Eugeny/tabby/discussions/9602
[x11-reddit]: https://reddit.birdcat.cafe/r/selfhosted/comments/1la9mw2/software_for_managing_ssh_connections_and_x11
[tabby-x11]: https://lightrun.com/answers/eugeny-tabby-x-forwarding-doesnt-work-properly
[putty-fido]: https://ftp.cvut.cz/putty/wishlist/fido2.html
[termius-fido]: https://termius.com/documentation/fido2
[dbeaver-1156]: https://github.com/dbeaver/dbeaver/issues/1156
[devart]: https://www.devart.com/dbforge/best-postgresql-clients.html
[vibe]: https://vibereference.vercel.app/backend-and-data/database-gui-tools
[mosh]: https://www.howtogeek.com/devops/what-is-the-mosh-shell-and-how-do-you-use-it/
[g2-dbeaver]: https://www.g2.com/products/dbeaver
[dbvis-alt]: https://www.dbvis.com/thetable/best-dbeaver-alternatives-of-2025/
[tabby-heavy]: https://x-cmd.com/install/tabby/
[pga-clicks]: https://postgrespro.com/list/thread-id/2509608
[tp-leak]: https://tableplus.com/blog/2017/06/how-using-ssh-tunneling-in-database-tools-might-leak-your-connection-forever.html
[tabby-hostkey]: https://newreleases.io/project/github/Eugeny/tabby/release/v1.0.171
[cg-sp]: https://github.com/conghuahuadan/ShellPort
[sn-sp]: https://github.com/Snuffy2/shellport
[sn-config]: https://github.com/Snuffy2/shellport/blob/main/CONFIGURATION.md
[ig-sp]: https://github.com/igorls/shellport
[sq-sp]: https://github.com/siqi2000/ShellPort
[tabby]: https://github.com/Eugeny/tabby
[windterm]: https://github.com/kingToolbox/WindTerm
[termix]: https://github.com/Termix-SSH/Termix
[electerm]: https://github.com/electerm/electerm
[xpipe]: https://github.com/xpipe-io/xpipe
[netcatty]: https://github.com/binaricat/Netcatty
[tssh]: https://github.com/trzsz/trzsz-ssh
[nyaterm]: https://github.com/nyakang/nyaterm
[omnyssh]: https://github.com/timhartmann7/omnyssh
[voltius]: https://github.com/VoltiusApp/voltius
[purple]: https://github.com/erickochen/purple
[rssh]: https://github.com/rssh-org/rssh
[opsdeck]: https://github.com/LeoAlecksey/opsdeck
