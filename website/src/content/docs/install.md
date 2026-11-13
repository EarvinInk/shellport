---
title: Installation
description: Install Shellport on Ubuntu, Debian, Linux Mint or any other distribution.
section: Getting started
order: 2
---

Shellport runs on 64-bit Linux (x86_64). It's developed and tested on Linux Mint 22, and should work on Ubuntu 22.04+, Debian 12+ and other current distributions that ship **WebKitGTK 4.1**.

Download the latest version from the [Download page](../../download/). There are two formats:

| File | Best for | Size |
|---|---|---|
| `Shellport_<version>_amd64.deb` | Ubuntu, Debian, Linux Mint, Pop!_OS | ~7 MB |
| `Shellport_<version>_amd64.AppImage` | Any distribution, no install needed | ~85 MB |

## Option A: .deb package

Recommended on Ubuntu, Debian and Linux Mint.

1. Download `Shellport_<version>_amd64.deb`.
2. Open a terminal in your Downloads folder and run:

   ```bash
   sudo apt install ./Shellport_*_amd64.deb
   ```

   apt installs the two system libraries Shellport needs (WebKitGTK and GTK) if they aren't already present.
3. Open **Shellport** from your applications menu, or run `shellport` in a terminal.

To update, download the newer `.deb` and run the same command.

## Option B: AppImage

Works on any distribution.

1. Download `Shellport_<version>_amd64.AppImage`.
2. Make it executable and run it:

   ```bash
   chmod +x Shellport_*_amd64.AppImage
   ./Shellport_*_amd64.AppImage
   ```

   You can also right-click the file → **Properties** → **Permissions** → *Allow executing file as program*, then double-click it.

> **AppImage won't start?** AppImages need FUSE 2. Install it with `sudo apt install libfuse2t64` (Ubuntu 24.04+) or `sudo apt install libfuse2` (Ubuntu 22.04 / Debian).

## Verify your download

Releases include a `SHA256SUMS` file. Download it into the same folder as the package and run:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

It should print `OK` next to the file you downloaded.
