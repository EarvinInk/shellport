---
title: Encrypted vault
description: Store passwords, private keys and API tokens behind a master password.
section: Features
order: 13
---

The vault keeps **passwords, private keys and API tokens** in one encrypted file, protected by a master password.

## Creating the vault

Open the **Vault** (shield icon) and choose a master password. It **cannot be recovered**: if you lose it, the vault's contents are gone.

## SSH keys

**Generate ed25519 keys** in the vault and copy the public key for a server's `authorized_keys`. Hosts can then sign in with a key stored in the vault.

## Auto-lock

The vault **locks automatically** after a period of inactivity. Change the timeout under **Settings**.

## Remember the vault

Optionally store the vault key in your **login keyring** (GNOME Keyring / KWallet) so it unlocks when you log in.

> Any program running as you can read the keyring while it's unlocked, so leave this off on shared machines.

## Backup & restore

In **Settings → Backup & restore** you can save everything to one encrypted file: hosts, database connections and settings, plus vault secrets if you tick **Include vault secrets**. The file is protected by a passphrase you choose (Argon2id + XChaCha20-Poly1305, like the vault), separate from your master password.

To restore, choose the file, enter its passphrase and click **Check** to see what would be added. Restoring only adds items you don't have yet; existing ones are kept as they are. To restore secrets, create or unlock the vault first. Tick **Also replace my settings** to take the backup's settings too.
