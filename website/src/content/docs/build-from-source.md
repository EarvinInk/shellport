---
title: Build from source
description: Compile Shellport yourself with Rust, Node.js and pnpm.
section: Reference
order: 24
---

You need Rust (stable), Node.js 20+ with pnpm, and the WebKitGTK development packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

Then clone and build:

```bash
git clone https://github.com/EarvinInk/shellport.git
cd shellport
pnpm install
pnpm tauri dev                                   # run in development
pnpm tauri build --bundles deb,appimage          # → src-tauri/target/release/bundle/
```

Run the tests with `cd src-tauri && cargo test`. A full release build needs about 5 GB of free disk space.

## About official builds

Official downloads are built by GitHub Actions on Ubuntu 22.04, so they run on that release and newer. Packages you build yourself only run on your distribution version or newer.
