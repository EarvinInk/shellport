---
title: Port forwarding
description: Reach remote services through local SSH tunnels.
section: Features
order: 14
---

Shellport supports local port forwarding (`ssh -L`) per host.

## Add a forward

1. Hover a host and click the **⇄** icon.
2. Add a forward, for example local port `15432` → `127.0.0.1:5432`.
3. Connect to `localhost:15432` from your database client to reach the remote database.

For PostgreSQL you can skip this: the [Databases](../databases/) panel opens its own tunnel.

## Auto-start

Tick **auto-start** to open the tunnel automatically every time you connect to that host.
