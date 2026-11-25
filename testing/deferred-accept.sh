#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")/.."

DAEMON=${FERRY_DAEMON_BIN:-./target/debug/ferry-daemon}
CLI=${FERRY_CLI_BIN:-./target/debug/ferry}
PORT_A=${FERRY_PORT_A:-48401}
PORT_B=${FERRY_PORT_B:-48402}
PAIR_PORT=${FERRY_PAIR_PORT:-49755}
RECV_TIMEOUT=${FERRY_RECV_TIMEOUT:-45}
TTL_SECS=${FERRY_TTL_SECS:-25}

for tool in dbus-daemon gnome-keyring-daemon; do
  command -v "$tool" >/dev/null || { echo "SKIP: $tool not installed — cannot run the invariant suite"; exit 0; }
done
[ -x "$DAEMON" ] || { echo "FAIL: $DAEMON not found — run: cargo build -p ferry-daemon -p ferry-cli"; exit 1; }
[ -x "$CLI" ] || { echo "FAIL: $CLI not found — run: cargo build -p ferry-daemon -p ferry-cli"; exit 1; }

ROOT=$(mktemp -d /tmp/ferry-verify-invariants.XXXXXX)
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT

FAILED=0
ok()   { echo "  ok:   $*"; }
bad()  { echo "  FAIL: $*"; FAILED=1; }
skip() { echo "  skip: $*"; }

start_session() {
  local name=$1 port=${2:-}
  local dir="$ROOT/$name"
  mkdir -p "$dir/xdg/ferry"
  [ -n "$port" ] && printf 'listen_port = %s\nauto_accept_from_roster = false\n' "$port" > "$dir/xdg/ferry/config.toml"

  local bus="unix:path=$dir/bus"
  dbus-daemon --session --address="$bus" --nofork --nopidfile >/dev/null 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 40); do [ -S "$dir/bus" ] && break; sleep 0.1; done
  [ -S "$dir/bus" ] || { echo "FAIL: $name session bus never came up"; exit 1; }

  DBUS_SESSION_BUS_ADDRESS="$bus" HOME="$dir" \
    sh -c 'printf "\n" | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1 &'
  for _ in $(seq 1 40); do
    if DBUS_SESSION_BUS_ADDRESS="$bus" dbus-send --session --dest=org.freedesktop.DBus \
        --type=method_call --print-reply /org/freedesktop/DBus \
        org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true"; then
      break
    fi
    sleep 0.25
  done
}

start_peer() {
  local name=$1 port=$2
  local dir="$ROOT/$name"
  local bus="unix:path=$dir/bus"
  start_session "$name" "$port"

  DBUS_SESSION_BUS_ADDRESS="$bus" HOME="$dir" XDG_DATA_HOME="$dir/xdg" \
    "$DAEMON" > "$dir/daemon.log" 2>&1 &
  local daemon_pid=$!
  PIDS+=("$daemon_pid")
  echo "$daemon_pid" > "$dir/daemon.pid"
}

stop_peer() {
  local dir="$ROOT/$1" sig="${2:-TERM}"
  if [ -f "$dir/daemon.pid" ]; then
    kill "-$sig" "$(cat "$dir/daemon.pid")" 2>/dev/null || true
    rm -f "$dir/daemon.pid"
  fi
  pkill -f "XDG_DATA_HOME=$dir/xdg" 2>/dev/null || true
  sleep 1
}

peer() {
  local name=$1; shift
  local dir="$ROOT/$name"
  DBUS_SESSION_BUS_ADDRESS="unix:path=$dir/bus" HOME="$dir" XDG_DATA_HOME="$dir/xdg" "$@"
}

wait_delivered() {
  local name=$1 needle=$2 out
  for _ in $(seq 1 "$RECV_TIMEOUT"); do
    out=$(peer "$name" "$CLI" receive list 2>/dev/null \
      | awk -v n="$needle" 'tolower($0) ~ tolower(n) && tolower($0) ~ /delivered/ {print $1}' | head -1 || true)
    [ -n "$out" ] && { echo "$out"; return 0; }
    sleep 1
  done
  return 1
}

echo "==> starting two isolated daemons (separate keychains, ports $PORT_A / $PORT_B)"
start_peer a "$PORT_A"
start_peer b "$PORT_B"
for name in a b; do
  ready=""
  for _ in $(seq 1 40); do peer "$name" "$CLI" status >/dev/null 2>&1 && { ready=1; break; }; sleep 0.5; done
  [ -n "$ready" ] || { echo "FAIL: $name daemon never became ready"; exit 1; }
done
ok "both daemons up"

echo
echo "== pairing (b listens, a connects) =="
peer b sh -c "printf 'y\n' | '$CLI' pair listen --bind 127.0.0.1:$PAIR_PORT --name peer-b > '$ROOT/b/pair.log' 2>&1" &
PIDS+=($!)
code=""
for _ in $(seq 1 40); do
  code=$(grep -oE "connect 127\.0\.0\.1:$PAIR_PORT [0-9]{6}" "$ROOT/b/pair.log" 2>/dev/null | grep -oE '[0-9]{6}$' | head -1 || true)
  [ -n "$code" ] && break
  sleep 0.25
done
[ -n "$code" ] || { echo "FAIL: b never printed a pairing code"; exit 1; }
peer a sh -c "printf 'y\n' | '$CLI' pair connect 127.0.0.1:$PAIR_PORT $code --name peer-a" >/dev/null 2>&1 \
  || { echo "FAIL: a could not complete pairing"; exit 1; }
sleep 1
peer a "$CLI" roster list | grep -qi peer-b || { echo "FAIL: a's roster missing b"; exit 1; }
BID=$(peer a "$CLI" roster list | awk 'tolower($0) ~ /peer-b/ {print $1}' | head -1)
ok "paired — a knows b as $BID"


echo "== deferred accept: delivered within seconds of the accept, and the notify receipt is acked =="
printf 'hold-me-%s\n' "$(date +%s%N)" > "$ROOT/hold.txt"
peer a "$CLI" send "$BID" "$ROOT/hold.txt" --ttl 3600 --notify-on-open >/dev/null 2>&1
item=""
for _ in $(seq 1 40); do item=$(peer b "$CLI" inbox list 2>/dev/null | awk 'tolower($0) ~ /hold.txt/ {print $1}' | head -1); [ -n "$item" ] && break; sleep 1; done
[ -n "$item" ] && ok "b sees the offer waiting" || bad "offer never appeared"
echo "     letting backoff grow for 40s"; sleep 40
T0=$(date +%s)
peer b "$CLI" inbox accept "$item" >/dev/null 2>&1
for _ in $(seq 1 120); do peer b "$CLI" receive list 2>/dev/null | grep -i hold.txt | grep -qi delivered && break; sleep 1; done
DT=$(( $(date +%s) - T0 ))
echo "     delivered ${DT}s after accept"
[ "$DT" -le 20 ] && ok "delivered ${DT}s after the accept (was up to 300s)" || bad "still ${DT}s after accept"
peer b "$CLI" receive open "$item" --out "$ROOT/hold.got" >/dev/null 2>&1
for _ in $(seq 1 40); do peer a "$CLI" sent list 2>/dev/null | grep -i hold.txt | grep -qi opened && break; sleep 1; done
peer a "$CLI" sent list 2>/dev/null | grep -i hold.txt | grep -qi opened && ok "open receipt reached the sender" || bad "receipt lost"

if [ "${FAILED:-0}" -ne 0 ]; then echo "==> DEFERRED-ACCEPT CHECKS FAILED"; exit 1; fi
echo "==> DEFERRED-ACCEPT CHECKS PASSED"
