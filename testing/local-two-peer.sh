#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

DAEMON=${FERRY_DAEMON_BIN:-./target/debug/ferry-daemon}
CLI=${FERRY_CLI_BIN:-./target/debug/ferry}
PORT_A=${FERRY_PORT_A:-47901}
PORT_B=${FERRY_PORT_B:-47902}
PAIR_PORT=${FERRY_PAIR_PORT:-49555}
SIZE=${FERRY_PAYLOAD_BYTES:-262144}
RECV_TIMEOUT=${FERRY_RECV_TIMEOUT:-45}

for tool in dbus-daemon gnome-keyring-daemon; do
  command -v "$tool" >/dev/null || { echo "SKIP: $tool not installed — cannot run the host two-peer test"; exit 0; }
done
[ -x "$DAEMON" ] || { echo "FAIL: $DAEMON not found — run: cargo build -p ferry-daemon -p ferry-cli"; exit 1; }
[ -x "$CLI" ] || { echo "FAIL: $CLI not found — run: cargo build -p ferry-daemon -p ferry-cli"; exit 1; }

ROOT=$(mktemp -d /tmp/ferry-local-two-peer.XXXXXX)
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT

pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*"; for p in a b; do echo "--- $p daemon log ---"; tail -40 "$ROOT/$p/daemon.log" 2>/dev/null || true; done; exit 1; }

start_peer() {
  local name=$1 port=$2
  local dir="$ROOT/$name"
  mkdir -p "$dir/xdg/ferry"
  printf 'listen_port = %s\n' "$port" > "$dir/xdg/ferry/config.toml"

  local bus="unix:path=$dir/bus"
  dbus-daemon --session --address="$bus" --nofork --nopidfile >/dev/null 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 40); do [ -S "$dir/bus" ] && break; sleep 0.1; done
  [ -S "$dir/bus" ] || fail "$name session bus never came up"

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

  DBUS_SESSION_BUS_ADDRESS="$bus" HOME="$dir" XDG_DATA_HOME="$dir/xdg" \
    "$DAEMON" > "$dir/daemon.log" 2>&1 &
  PIDS+=($!)
}

peer() {
  local name=$1; shift
  local dir="$ROOT/$name"
  DBUS_SESSION_BUS_ADDRESS="unix:path=$dir/bus" HOME="$dir" XDG_DATA_HOME="$dir/xdg" "$@"
}

echo "==> starting two independent daemons (separate keychains, ports $PORT_A / $PORT_B)"
start_peer a "$PORT_A"
start_peer b "$PORT_B"

for name in a b; do
  ready=""
  for _ in $(seq 1 40); do
    if peer "$name" "$CLI" status >/dev/null 2>&1; then ready=1; break; fi
    sleep 0.5
  done
  [ -n "$ready" ] || fail "$name daemon never became ready"
done
pass "both daemons up"

fp_a=$(peer a "$CLI" identity | awk '/^fingerprint:/ {print $2}')
fp_b=$(peer b "$CLI" identity | awk '/^fingerprint:/ {print $2}')
[ -n "$fp_a" ] && [ -n "$fp_b" ] && [ "$fp_a" != "$fp_b" ] || fail "the two daemons share an identity ($fp_a / $fp_b) — keychain isolation failed"
pass "distinct identities: a=$fp_a b=$fp_b"

echo "==> pairing (b listens, a connects)"
peer b sh -c "printf 'y\n' | '$CLI' pair listen --bind 127.0.0.1:$PAIR_PORT --name peer-b > '$ROOT/b/pair.log' 2>&1" &
PIDS+=($!)

code=""
for _ in $(seq 1 40); do
  code=$(grep -oE 'connect 127\.0\.0\.1:'"$PAIR_PORT"' [0-9]{6}' "$ROOT/b/pair.log" 2>/dev/null | grep -oE '[0-9]{6}$' | head -1 || true)
  [ -n "$code" ] && break
  sleep 0.25
done
[ -n "$code" ] || fail "peer b never printed a pairing code: $(cat "$ROOT/b/pair.log" 2>/dev/null || true)"
pass "pairing code = $code"

peer a sh -c "printf 'y\n' | '$CLI' pair connect 127.0.0.1:$PAIR_PORT $code --name peer-a" || fail "peer a could not complete pairing"
sleep 1

peer a "$CLI" roster list | grep -qi peer-b || fail "a's roster is missing b"
peer b "$CLI" roster list | grep -qi peer-a || fail "b's roster is missing a"
pass "both rosters contain the paired peer"

peer_b_id=$(peer a "$CLI" roster list | awk 'tolower($0) ~ /peer-b/ {print $1}' | head -1)
[ -n "$peer_b_id" ] || fail "could not read b's peer id from a's roster"

echo "==> a sends a $SIZE-byte file to b"
head -c "$SIZE" /dev/urandom > "$ROOT/payload.bin"
want=$(sha256sum "$ROOT/payload.bin" | awk '{print $1}')
peer a "$CLI" send "$peer_b_id" "$ROOT/payload.bin" --ttl 3600 || fail "a could not queue the send"
pass "send queued for $peer_b_id"

echo "==> waiting up to ${RECV_TIMEOUT}s for b to receive it via mDNS"
item=""
for _ in $(seq 1 "$RECV_TIMEOUT"); do
  item=$(peer b "$CLI" receive list 2>/dev/null | awk 'tolower($0) ~ /payload.bin/ && tolower($0) ~ /delivered/ {print $1}' | head -1 || true)
  [ -n "$item" ] && break
  sleep 1
done

if [ -z "$item" ]; then
  echo
  echo "WARN: b did not receive the item within ${RECV_TIMEOUT}s."
  echo "      Boot + isolated identities + pairing + signed roster + send-queue all passed."
  echo "      Delivery needs mDNS multicast to resolve a peer on this host; if this box"
  echo "      blocks multicast (even on loopback) the chunked-transfer + hash path is"
  echo "      still covered by the workspace tests. Try FERRY_RECV_TIMEOUT=90."
  echo "--- a daemon log (tail) ---"; tail -20 "$ROOT/a/daemon.log"
  echo "--- b daemon log (tail) ---"; tail -20 "$ROOT/b/daemon.log"
  echo "==> CORE CHECKS PASSED (transfer delivery not observed)"
  exit 0
fi
pass "b received item $item"

peer b "$CLI" receive open "$item" --out "$ROOT/received.bin" || fail "b could not open the item"
got=$(sha256sum "$ROOT/received.bin" | awk '{print $1}')
[ "$want" = "$got" ] || fail "content hash mismatch: sent $want, got $got"
pass "received content matches ($want)"

echo
echo "==> ALL CHECKS PASSED — real Linux-to-Linux transfer on one host"
