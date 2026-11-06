#!/bin/sh
# Test-only setup for the disposable sshd container:
# let user 'test' reach the host Docker socket and allow SSH forwarding.
DOCKER_GID="${DOCKER_GID:-139}"
apk add --no-cache docker-cli >/dev/null 2>&1 || true
addgroup -g "$DOCKER_GID" dockerhost 2>/dev/null || true
G=$(getent group "$DOCKER_GID" | cut -d: -f1)
addgroup test "$G" 2>/dev/null || true
CFG=/config/sshd/sshd_config
if [ -f "$CFG" ]; then
  sed -i 's/^AllowTcpForwarding.*/AllowTcpForwarding yes/' "$CFG"
  grep -q '^AllowStreamLocalForwarding' "$CFG" || echo 'AllowStreamLocalForwarding yes' >> "$CFG"
fi
