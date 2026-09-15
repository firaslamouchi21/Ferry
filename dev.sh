#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

echo "==> building ferry-daemon + ferry (debug, incremental — fast after the first run)"
cargo build -p ferry-daemon -p ferry-cli

echo "==> installing JS dependencies"
pnpm install

echo "==> starting the UI dev server"
echo "    open http://localhost:5173 — if it shows 'daemon not running', click Start daemon"
exec pnpm --filter @ferry/ui dev
