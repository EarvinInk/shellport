---
title: Databases
description: Browse and query PostgreSQL directly or through an SSH host.
section: Features
order: 15
---

Shellport has a built-in database client. PostgreSQL is supported today; more engines can be added as drivers.

## Add a connection

1. Click the **cylinder** icon on the left rail, then **+**.
2. Enter the **host**, **port**, **database** and **username**.
3. Pick a **stored password** from the vault, or type a new one. It is saved to the vault, never to the profile file.
4. Optional: under **SSH tunnel**, choose one of your saved hosts. The host and port you entered are then resolved *from that server*, so a database listening on the server's `127.0.0.1:5432` is reachable without exposing it.
5. Click **Test connection**, then **Save**.

The tunnel uses a random local port that only Shellport uses, and it closes with the tab. Jump hosts configured on the SSH host are used automatically.

## Browse and query

Double-click a saved connection to open it in a tab.

- The **schema tree** on the left lists schemas, then tables, views and functions, then columns with their types and primary keys. Double-click a table or view to preview its first 100 rows.
- Write SQL in the editor and press **Ctrl+Enter**. If text is selected, only the selection runs. Scripts with several statements show one result tab per statement.
- **Cancel** stops a long-running query on the server.
- **Row limit** caps how many rows each statement keeps; the status bar says when more rows existed.
- Click a cell and press **Ctrl+C** (or double-click it) to copy it. **Copy TSV** copies the whole result for pasting into a spreadsheet.
- Server notices (e.g. `NOTICE` from `RAISE`) appear in the status bar.

Your editor text is kept per connection between sessions.

## TLS

| Mode | Behaviour |
|---|---|
| Disable | No encryption. |
| Prefer | Encrypt if the server offers it; the certificate is not checked. |
| Require | Always encrypt; the certificate is not checked. |
| Verify full | Always encrypt, and check the certificate against public CAs and the host name. |

When tunnelling over SSH the traffic is already encrypted between you and the SSH server, so *Prefer* is usually enough.
