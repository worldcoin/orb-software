#!/bin/sh
set -eu

# Create numeric-matching user/group (idempotent-ish)
groupadd -g "$TARGET_GID" host 2>/dev/null || true
id -u host >/dev/null 2>&1 || useradd -u "$TARGET_UID" -M -N -g "$TARGET_GID" -s /usr/sbin/nologin host

# Ensure the bind-mount dir is owned by host uid:gid
chown -R "$TARGET_UID:$TARGET_GID" /run/integration-tests

echo "starting dbus"
dbus-daemon --fork --config-file=/etc/dbus.conf --print-address

# Wait for dbus to create the socket, then set owner/perms
for i in $(seq 1 50); do
  [ -S /run/integration-tests/socket ] && break
  sleep 0.1
done

chown "$TARGET_UID:$TARGET_GID" /run/integration-tests/socket
chmod 660 /run/integration-tests/socket

echo "starting zenoh"
zenohd --config=/etc/zenohd.json5 &

echo "starting network-manager"
exec /usr/sbin/NetworkManager --no-daemon
