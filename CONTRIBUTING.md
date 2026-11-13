# Contributing to Shellport

Thanks for your interest in Shellport! Bug reports, ideas, documentation fixes and code are all welcome. This guide explains how to get set up and what to expect when you contribute.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to help

- **Report a bug.** Use the [bug report form](https://github.com/EarvinInk/shellport/issues/new?template=bug_report.yml). Include your distribution, Shellport version and the steps to reproduce.
- **Suggest a feature.** Open a [feature request](https://github.com/EarvinInk/shellport/issues/new?template=feature_request.yml), or start a thread in [Discussions → Ideas](https://github.com/EarvinInk/shellport/discussions/categories/ideas). Check the [roadmap](ROADMAP.md) first; it may already be planned.
- **Ask or answer a question** in [Discussions → Q&A](https://github.com/EarvinInk/shellport/discussions/categories/q-a).
- **Improve the docs**: the [README](README.md) or the website docs in `website/src/content/docs/`.
- **Send code.** See below.

**Security issues** must not be reported in public issues or pull requests. Follow the [security policy](SECURITY.md) instead.

## Before you write code

For anything bigger than a small fix, please **open an issue or discussion first** so we can agree on the approach. It saves you from building something that doesn't fit the project.

Shellport follows a few principles (see the [roadmap](ROADMAP.md)), and changes are expected to respect them:

- **Local-first and private.** No telemetry, no accounts, no network calls except to servers the user chose.
- **Light and fast.** Keep the UI responsive, including with large output or big result sets.
- **Safe on production.** Destructive actions confirm first; host keys and secrets are never handled loosely.

## Development setup

You need Rust (stable), Node.js 20+ with pnpm, and the WebKitGTK development packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

Then:

```bash
git clone https://github.com/<your-username>/shellport.git
cd shellport
pnpm install
pnpm tauri dev
```

### Test servers

`dev/docker-compose.test.yml` starts disposable SSH, PostgreSQL and PgBouncer targets. See *Test servers and project layout* at the end of the [README](README.md#build-from-source) for ports, credentials and the live tests that use them.

> **Keep your real data safe while testing.** Point test runs at a separate config folder with `SHELLPORT_CONFIG_DIR=/path/to/test-config pnpm tauri dev`, so your own hosts and vault (in `~/.config/shellport/`) aren't touched.

### Project layout

| Path | What lives there |
|---|---|
| `src-tauri/src/` | Rust core: SSH, SFTP, Docker, database drivers, vault, stores, and `commands.rs` (the API the UI calls) |
| `src/` | React UI: `views/`, `components/`, and `lib/ipc.ts` (typed calls into the Rust side) |
| `website/` | The project website (Astro), published to GitHub Pages |
| `dev/` | Test containers and the icon generator |

## Checks to run

CI runs these on every pull request, so run them locally first:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check          # Rust formatting
pnpm build                                                    # type-check and bundle the UI
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked  # Rust unit tests
```

If the formatting check fails, run `cargo fmt --manifest-path src-tauri/Cargo.toml` to fix it.

If you changed the website:

```bash
pnpm --dir website install
pnpm --dir website build
```

Please also try your change in the running app (`pnpm tauri dev`). Many features only show their problems against a real server.

## Making a pull request

1. Fork the repository and create a branch from `main` (e.g. `fix-sftp-symlink`).
2. Keep the change focused: one fix or feature per pull request.
3. Add or update tests where it makes sense, especially for parsing, security checks and data handling.
4. Add a line under **Unreleased** in [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.
5. Update the README or website docs if behaviour or options changed.
6. Open the pull request and fill in the template. Screenshots or a short recording help a lot for UI changes.

### Style

- **Rust code must be formatted with `cargo fmt`** (default rustfmt settings). CI rejects pull requests that aren't. Set your editor to format on save, or run it before committing.
- For the TypeScript UI, follow the style of the code around you, and don't reformat code you aren't otherwise changing.
- Commit messages: a short, capitalised summary in the imperative mood ("Fix SFTP upload of empty folders"), with more detail in the body if needed.
- **Screenshots and examples must use demo data only**: no real hostnames, IP addresses, usernames or container names. Documentation IP ranges (`192.0.2.x`, `198.51.100.x`, `203.0.113.x`) and names like `dev-box` work well.

## Review

The maintainer reviews pull requests as time allows. You may be asked for changes; that's normal and not a rejection. Once approved, the pull request is merged into `main` and ships in the next release.

## License

Shellport is [MIT licensed](LICENSE). By contributing, you agree that your contributions are licensed under the same terms.
