# Shellport

A native Linux SSH client in the spirit of Terminus/Tabby, built with Tauri 2 (Rust) + React.

- **Terminals**: tabbed local shells, SSH sessions, `docker exec` shells (xterm.js, search, themes, reconnect)
- **Secrets vault**: Argon2id + XChaCha20-Poly1305 encrypted file, optional unlock via the system keyring (GNOME Keyring / KWallet), auto-lock, ed25519 key generation, public-key export
- **Hosts**: groups, colors, password / vault key / key file / ssh-agent auth, ProxyJump, startup command, local port forwards, import from `~/.ssh/config` (incl. `Include`, `Host *`, `ProxyJump`)
- **Host key verification**: checks `~/.ssh/known_hosts`, prompts on unknown keys, hard warning on changed keys
- **SFTP / SCP**: dual-pane local ↔ remote browser, recursive upload/download with progress, drag & drop (also from the desktop), mkdir/rename/chmod/delete
- **Docker**: local daemon or any SSH host. List containers (grouped by compose project) and images, start/stop/restart/pause/kill/remove, inspect, follow logs, open a shell. Remote Docker uses the remote socket tunnelled over SSH (`direct-streamlocal`), falling back to the `docker` CLI over SSH.

## Layout

```
src-tauri/src/
  vault/        encrypted vault + keyring integration
  store/        hosts.json, settings.json, ~/.ssh/config parser
  ssh/          connections, auth, host keys, ProxyJump, port forwards
  term.rs       local PTY / SSH shell / docker exec sessions
  sftp.rs       file browsing and transfers
  docker.rs     local + remote Docker (socket tunnel or CLI fallback)
  commands.rs   Tauri command surface
src/            React UI (views/, components/, lib/ipc.ts)
dev/            disposable sshd + log container for testing
```

Config lives in `~/.config/shellport/` (`hosts.json`, `settings.json`, `vault.bin`, all mode 0600).

## Develop

System packages (once):

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

```bash
pnpm install
pnpm tauri dev
```

Tests:

```bash
cd src-tauri && cargo test
```

Test targets (SSH on `127.0.0.1:2222`, user `test`, password `shellport-test` or `dev/test_ed25519`):

```bash
ssh-keygen -t ed25519 -N "" -f dev/test_ed25519   # first time only
docker compose -f dev/docker-compose.test.yml up -d
cd src-tauri && cargo run --example smoke          # headless protocol smoke test
```

## Build packages

```bash
pnpm tauri build    # → src-tauri/target/release/bundle/{deb,appimage}
```

## Shortcuts

Ctrl+Shift+T new local tab · Ctrl+Shift+W close tab · Ctrl+PgUp/PgDn switch tabs · Ctrl+Shift+C/V copy/paste · Ctrl+Shift+F search in terminal
