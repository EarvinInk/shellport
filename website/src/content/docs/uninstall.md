---
title: Uninstall
description: Remove Shellport and, optionally, your data.
section: Reference
order: 23
---

## .deb package

```bash
sudo apt remove shellport
```

## AppImage

Just delete the file.

## Removing your data

Your hosts, settings and vault stay in `~/.config/shellport/`. Delete that folder too if you want to remove everything.

> Deleting `~/.config/shellport/` **permanently deletes your vault**.
