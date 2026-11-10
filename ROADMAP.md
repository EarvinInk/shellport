# Shellport roadmap

This roadmap is built from public complaints and feature requests about other SSH and database clients: DBeaver, pgAdmin, TablePlus, Beekeeper Studio, DataGrip, MySQL Workbench, DbVisualizer, Termius, Tabby, electerm, MobaXterm, PuTTY and Remote Desktop Manager. Each item links to the discussion that motivated it. Researched November 2026.

**Principles that shape the order**
- **Local-first and private.** No account, no subscription and no telemetry. Paywalled sync and cloud storage are among the most common Termius complaints, so anything shared between devices stays optional and self-hosted.
- **Light and fast.** Slowness and memory use are DBeaver's most common review complaints, and freezing on large output is a recurring Tabby and DbVisualizer report. Every feature must keep the app responsive.
- **Safe on production.** Shellport is used against real servers, so mistakes that can't be undone get guard rails before convenience features do.
- **Fix what is shipped before adding more.**

**Status:** ✅ marks items that are built (see the [changelog](CHANGELOG.md), *Unreleased*).

**Size:** S is about a day, M a few days, L a week or more. **Area:** DB (database client), SSH (terminal and connections), App (both).

---

## Phase 0: fix what is shipped (database client)

These affect the PostgreSQL client that is already built.

| # | Item | Why | Size |
|---|------|-----|------|
| 0.1 ✅ | **Work behind connection poolers.** Use unnamed statements (`query_typed`) instead of named prepared statements for queries and schema lookups. | Behind PgBouncer or Supabase's pooler in transaction mode, named prepared statements fail with `prepared statement "s0" does not exist` ([DataGrip thread][dg-pgb], [Supabase][supa-prep]). Our driver currently prepares every query this way. | S |
| 0.2 ✅ | **A separate draft per tab.** Save editor text per tab, not per database. | Two tabs on the same database currently overwrite each other's draft, and losing query text is a long-standing complaint ([Workbench #109360][wb-109360], [pgAdmin #366][pga-366]). | S |
| 0.3 ✅ | **Large results that don't freeze.** Draw only the visible rows, and stop reading from the server once the row limit is reached. | Big grids freezing the GUI is a top complaint ([DbVisualizer][dbvis-slow], [Workbench #67717][wb-67717]). We cap rows but still draw all of them and still download everything the server sends. | M |
| 0.4 ✅ | **Survive dropped connections.** Turn on TCP keep-alives for database connections, detect a lost session, and offer "Reconnect" (optionally automatic) while keeping the editor text. | Beekeeper tracks SSH tunnels that drop and don't reconnect as a bug ([#2165][bk-issues]); Metabase had to fix tunnel reconnects ([#6114][mb-6114]). | M |

## Phase 1: safety and reliability

| # | Area | Item | Why | Size |
|---|------|------|-----|------|
| 1.1 ✅ | DB | **Production safety.** A read-only option per connection, enforced by the server (`default_transaction_read_only`). A confirmation before `UPDATE`/`DELETE` without `WHERE`, `DROP` or `TRUNCATE`. The connection colour shown as a stripe on its tab and editor. | Accidental `UPDATE` without `WHERE` is a classic ([MySQL safe updates][mysql-1175], [SQLAuthority][sqla]). TablePlus asks before running queries in [safe mode][tp-safe], and its users want it per connection tag ([TablePlus #2669][tp-2669]). | M |
| 1.2 ✅ | DB | **Open-transaction indicator.** Show "Transaction open" with Commit and Rollback buttons, and warn when closing a tab or disconnecting with an open transaction. | A forgotten `BEGIN` in a GUI holds locks and blocks cleanup on the server ([pganalyze][pga-idle], [Netdata][nd-idle]). DataGrip users asked for exactly this marker ([thread][dg-tx]). | M |
| 1.3 ✅ | SSH | **Automatic reconnect.** Reconnect after sleep or a network change, with backoff and a visible "reconnecting" state. A per-host choice between reconnect, ask and close tab. An option to run `tmux new -A -s shellport` on connect so work survives the drop. | Plain SSH dies when the IP changes or the laptop sleeps ([OpenReplay][or-unstable], [tmux guide][tmux]). Tabby users want reconnect behaviour configurable ([#8062][tabby-8062]); Warp users ask for it too ([#442][warp-442]). Today Shellport only has a manual Reconnect button. | M |
| 1.4 ✅ | SSH | **Responsive with heavy output.** Batch terminal writes and cap how often the screen redraws, so a runaway `cat` or a log flood never freezes the window. Test with a 100 MB stream. | Tabby reports [unresponsive tabs under fast output][tabby-slow]. | S |
| 1.5 ✅ | App | **Encrypted backup and restore.** Export hosts, database profiles, settings and (optionally) the vault to one encrypted file, and import it on another machine. | People leave Termius over paid sync and privacy ([Lemmy][lemmy-termius], [G2][g2-termius], [Terminator][term-sync]). An encrypted file the user controls covers moving machines without an account. | M |

## Phase 2: everyday productivity

| # | Area | Item | Why | Size |
|---|------|------|-----|------|
| 2.1 | DB | **Query history** per connection: searchable, with time, duration and row count, and one-click re-run. | Losing "recent queries" was a headline pgAdmin 4 complaint ([thread][pga-4v3]). | M |
| 2.2 | DB | **SQL editor upgrade** with CodeMirror 6: syntax highlighting; autocomplete for tables and columns that follows aliases and the tables in the query, using the schema we already load; a word-wrap toggle (Alt+Z). | Context-aware autocomplete is the most repeated editor request ([sqltools #110][sqltools-110], [Workbench #52917][wb-52917], [CodeMirror][cm-sql]). Beekeeper users asked for a wrap toggle ([r/SQL][bk-reddit]). | L |
| 2.3 | DB | **Paste a connection string.** Paste `postgres://user@host:5432/db?sslmode=require` and the fields fill themselves; any password goes straight into the vault. | Requested in [Antares #193][antares-193] and [sqlectron #716][sqlectron-716]; the Azure VS Code extension does this ([docs][azure-conn]). | S |
| 2.4 | DB | **Copy and export results** as CSV, JSON or INSERT statements, for the selected rows or all of them, to the clipboard or a file. | Beekeeper users asked to copy rows, not only download them ([r/SQL][bk-reddit]). | S |
| 2.5 | SSH | **Snippets.** Saved commands, global or per host or group, with variables (e.g. `{{service}}`), run from a palette (Ctrl+Shift+P) or sidebar. | electerm users want quick commands pinned ([issues][electerm-issues]); Remote Desktop Manager has snippets, including ones that run on connect ([forum][rdm-broadcast]). | M |
| 2.6 | SSH | **Split panes.** Several terminals in one tab, side by side or stacked, with drag-to-resize. | A common expectation from Tabby, iTerm2 and MobaXterm users ([AlternativeTo][alt-moba]). It is also the foundation for 2.7. | L |
| 2.7 | SSH | **Broadcast input.** Type once into several panes or tabs. You choose the target set, and a clear banner shows while it's on. | Long-standing requests in Remote Desktop Manager ([1][rdm-bc1], [2][rdm-bc2], [docs][rdm-broadcast]) and iTerm2 workflows ([i2cssh][i2cssh]). | M |
| 2.8 | SSH | **Session logging and save output.** "Save output to file" for a tab, optional automatic logs per host (plain text, with timestamps), and saving the scrollback. | Tabby needs a plugin for this ([save-output][tabby-save]); Termius has logs but no export ([docs][termius-logs]); RDM users want a one-click toggle ([forum][rdm-log]). | M |

## Phase 3: connection features

The SSH library we use (russh 0.65) already provides the pieces for 3.2 to 3.5, so most of the work is in the UI and settings.

| # | Area | Item | Why | Size |
|---|------|------|-----|------|
| 3.1 | SSH | **Dynamic forwarding (`-D`).** A local SOCKS5 proxy through a host, set up per host in the forwards dialog. | People can't replace this with plain port forwards ([electerm #3423][electerm-3423]); PuTTY tracks the related [request][putty-dyn]. | M |
| 3.2 | SSH | **Remote forwarding (`-R`).** Expose a local port on the server. | It completes the set of forwarding types; russh provides `tcpip_forward`. | M |
| 3.3 | SSH | **Agent forwarding**, per host, off by default and with a warning about trusting that server. | Expected by people who hop between servers. Tabby lists it, and users noticed when it broke on macOS ([discussion][tabby-agent]). | S |
| 3.4 | SSH | **X11 forwarding** per host (uses the local `$DISPLAY`). | A Termius user on Linux Mint left over it after years on Termius's roadmap ([r/selfhosted][x11-reddit]); Tabby users hit bugs with it ([issue][tabby-x11]). MobaXterm's X server is what Linux users miss most ([AlternativeTo][alt-moba]). | M |
| 3.5 | SSH | **OpenSSH certificates.** Use `id_*-cert.pub` alongside a key, and accept host certificates through `@cert-authority` in known_hosts. | Common in teams with an SSH CA. russh provides `authenticate_openssh_cert`. | M |
| 3.6 | SSH | **Hardware security keys (FIDO2, `sk-ed25519`).** First make sure they work through ssh-agent, then look into using them directly. | Many clients lack this: there are open requests for PuTTY ([wishlist][putty-fido]) and others, and Termius added it ([docs][termius-fido]). | M–L |
| 3.7 | DB | **Show a table's CREATE statement** from the right-click menu, with a copy button. | Requested by Beekeeper users ([r/SQL][bk-reddit]). | S |
| 3.8 | DB | **Catch host typos.** Trim spaces and stray `/` in host fields, and warn about obvious mistakes. | A trailing `/` broke DBeaver tunnels while its test still passed ([#1156][dbeaver-1156]). | S |

## Phase 4: bigger bets

| # | Area | Item | Notes | Size |
|---|------|------|-------|------|
| 4.1 | DB | **More engines:** MySQL/MariaDB, then SQLite (local, or over SFTP). | The driver design already supports this (see README, "Adding a database engine"). | L each |
| 4.2 | DB | **EXPLAIN view:** run `EXPLAIN (ANALYZE, BUFFERS)` and show the plan as a readable tree with costs and the slowest steps highlighted. | Mostly promoted in vendor articles ([Devart][devart]), but useful for Postgres users. | L |
| 4.3 | DB | **Edit rows in the grid,** with pending changes, a preview of the generated SQL, then Save or Discard. Builds on 1.1 and 1.2. | The feature TablePlus is known for ([overview][vibe]). Needs primary keys and careful safeguards. | L |
| 4.4 | SSH | **Mosh or Eternal Terminal sessions** for unreliable networks. | Mosh stays connected across network changes and sleep ([How-To Geek][mosh]); it needs UDP and the server-side program. | L |
| 4.5 | App | **Optional self-hosted sync** of the encrypted backup (1.5), e.g. a file in a folder you already sync. | Avoids the subscription model users dislike ([Lemmy][lemmy-termius], [Terminator][term-sync]). | L |
| 4.6 | SSH | **Serial console** sessions for routers and boards. | Offered by Tabby and MobaXterm; a niche need. | M |

## Already strengths: keep them

- **Light, native app** (Tauri). Heaviness is the most common complaint about DBeaver ([G2][g2-dbeaver], [DbVis][dbvis-alt]) and Tabby ([review][tabby-heavy]).
- **One double-click to a terminal, files or a database.** pgAdmin users disliked clicking through a server tree every time ([thread][pga-clicks]).
- **SSH tunnels run inside the app**, so they can't outlive it (compare TablePlus's [post on leaked tunnels][tp-leak]). Tunnel reachability is checked before connecting, with plain-language errors.
- **Free, open source and local:** no account or subscription, which is what people leaving Termius are looking for.
- **Host key verification, encrypted vault and 2FA logins** are already built in. Tabby only [added host key checking][tabby-hostkey] recently.

## Not planned

- Accounts, hosted cloud sync or anything paid. See the principles above.
- An AI assistant in the terminal. Not requested in the sources reviewed, and at odds with "private by default".

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
