## What does this change?

<!-- A short description of the change and why it's needed. -->

Closes #

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Improvement to an existing feature
- [ ] Documentation or website
- [ ] Build, CI or refactoring (no user-facing change)

## How was it tested?

<!-- What you ran and what you tried in the app, e.g. "connected to the test SSH container and uploaded a folder". -->

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` passes
- [ ] `pnpm build` passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked` passes
- [ ] Tried the change in the running app (`pnpm tauri dev`)

## Screenshots

<!-- For UI changes: before/after screenshots or a short recording. Use demo data only: no real hosts, IPs or usernames. -->

## Checklist

- [ ] I've read [CONTRIBUTING.md](https://github.com/EarvinInk/shellport/blob/main/CONTRIBUTING.md) and this PR is focused on one change
- [ ] User-facing changes are listed under **Unreleased** in `CHANGELOG.md`
- [ ] README or website docs are updated if behaviour or options changed
- [ ] No telemetry, new network calls or secrets in logs were added
