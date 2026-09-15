#!/usr/bin/env bash
set -euo pipefail

DATA_DIR="${XDG_DATA_HOME:-/data}"
mkdir -p "$DATA_DIR/ferry"
CONFIG="$DATA_DIR/ferry/config.toml"
if [ ! -f "$CONFIG" ]; then
  printf 'identity_keystore = "file"\n' >"$CONFIG"
fi

if [ -z "${FERRY_IDENTITY_PASSPHRASE:-}" ]; then
  echo "ferry: FERRY_IDENTITY_PASSPHRASE must be set — a headless container has no OS keychain," \
    "so the identity key is stored age-encrypted on disk instead, never unwrapped." >&2
  exit 1
fi

ferry-daemon &
daemon_pid=$!

shutdown() {
  kill -TERM "$daemon_pid" 2>/dev/null || true
  wait "$daemon_pid" 2>/dev/null || true
  exit 0
}
trap shutdown TERM INT

node /app/dev-bridge/serve.mjs &
server_pid=$!

wait -n "$daemon_pid" "$server_pid"
shutdown
