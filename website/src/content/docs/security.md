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

## Downloads and links

- Downloads never write outside the folder you choose and never follow symlinks, even if a server sends hostile file names.
- Links in terminal output open in your browser only after you confirm, and only `http(s)` links.

## Where your data lives

Your data lives in `~/.config/shellport/` (`hosts.json`, `settings.json`, `vault.bin`), readable only by you (mode `0600`).

## Reporting a vulnerability

Please report security issues privately via the [contact page](../../contact/) rather than a public issue.
