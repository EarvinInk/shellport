---
title: Security & privacy
description: How Shellport protects your servers, secrets and data.
section: Reference
order: 21
---

## No telemetry

**No telemetry, no accounts, no cloud.** Shellport only connects to the servers and Docker engines you tell it to.

## Vault encryption

Argon2id (64 MiB, 3 passes) derives the key from your master password; contents are encrypted with **XChaCha20-Poly1305**, and any tampering with the file is detected.

## Remembered vaults

Remembered vaults store the vault key in your login keyring. Any program running as you can read it while the keyring is unlocked, so leave this off on shared machines.

## Host keys

Host keys are verified against `~/.ssh/known_hosts` (hashed names, wildcards and `@revoked` entries included). New servers show their fingerprint before anything is sent. On a **changed key** Shellport sends no password or key unless you explicitly replace the saved key.

## Production guard rails

Mark a host as a **production server** and Shellport adds a confirmation wherever a slip would hurt:

- Databases reached through it treat risky SQL as *Confirm risky*, even when the connection is set to *Off*.
- Broadcast typing and **Run on many hosts** ask before including it, every time.
- Copying files to it, extracting archives on it and switching to root on it ask first.
- Multi-line pastes into its terminals ask first (by default).

## Database connections

- Passwords are kept in the vault, never in `databases.json`.
- An SSH tunnel uses a random local port that only Shellport uses, and closes with the tab.
- *Read-only* asks the server to refuse writes for the session. For a hard guarantee, also connect with a database role that can only read.
- After a dropped connection, Shellport reconnects but never re-runs your last query, because it might have been a write.

## Working as root

When you use *Work as root (sudo)* in the file browser, the sudo password is asked for once per session and **never stored**.

## Backups

Encrypted backups use a passphrase you choose, with the same encryption as the vault (Argon2id + XChaCha20-Poly1305). Vault secrets are included only if you tick **Include vault secrets**. Nothing is uploaded anywhere: the backup is a file you keep.

## History and logs

- **Activity** never contains passwords or other secrets, and stays on this computer.
- **Terminal logs** are off unless you turn them on for a host. They are plain text: anything shown in the terminal ends up in them (passwords you type aren't, because the terminal doesn't show them).

## Status checks

The up/down dots on hosts come from a short SSH greeting to each host's SSH port every few minutes while the window is visible, like `ssh-keyscan`. No login is attempted. Turn it off in Settings if you prefer.

## Downloads and links

- Downloads never write outside the folder you choose and never follow symlinks, even if a server sends hostile file names.
- Links in terminal output open in your browser only after you confirm, and only `http(s)` links.

## Where your data lives

Your data lives in `~/.config/shellport/`, which only you can read (folder mode `0700`, files `0600`):

| File | Holds |
|---|---|
| `hosts.json` | Saved hosts, groups and notes |
| `databases.json` | Database connections (passwords are in the vault) |
| `library.json` | The commands library and saved workspaces |
| `settings.json` | Settings |
| `session.json` | Open tabs and panes, restored on startup |
| `activity.json` | Activity history (queries, multi-host runs, transfers) |
| `vault.bin` | The encrypted vault |
| `logs/` | Terminal logs, only for hosts where you turn them on |
| `tmp/` | Copies of remote files open in your own editor |

## Reporting a vulnerability

Please report security issues privately via the [contact page](../../contact/) rather than a public issue.
