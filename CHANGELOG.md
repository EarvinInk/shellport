# Changelog

All notable changes to Shellport are listed here. Versions follow [semantic versioning](https://semver.org).

## [Unreleased]

### Added
- Databases: a built-in PostgreSQL client with saved connections, a schema browser, a SQL editor (Ctrl+Enter runs the script or the selection), a result grid, query cancel, row limits and server notices.
- Database connections can go through any saved SSH host (including jump hosts) over a private tunnel; passwords are kept in the vault.
- Database support is driver-based, so more engines can be added without changing the rest of the app.

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

