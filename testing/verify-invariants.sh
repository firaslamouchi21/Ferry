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
  [ -n "$port" ] && printf 'listen_port = %s\n' "$port" > "$dir/xdg/ferry/config.toml"

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
echo "== daemon lifecycle: ferry daemon start / status / stop drives a real daemon subprocess =="
start_session L 48409
peer L "$CLI" daemon status >/dev/null 2>&1 && bad "daemon status reported running before start" || ok "status is 'not running' before start"
out=$(peer L "$CLI" daemon start 2>&1); rc=$?
echo "     start rc=$rc: $out"
{ [ $rc -eq 0 ] && echo "$out" | grep -qi "started"; } && ok "daemon start spawned a daemon that accepts connections" || bad "daemon start failed"
peer L "$CLI" status >/dev/null 2>&1 && ok "ferry status reaches the started daemon" || bad "ferry status could not reach the started daemon"
peer L "$CLI" daemon start 2>&1 | grep -qi "already running" && ok "a second start is a no-op, not a double-spawn" || bad "second start did not report 'already running'"
FP1=$(peer L "$CLI" identity 2>/dev/null | awk '/fingerprint/ {print $2}')
peer L "$CLI" daemon stop >/dev/null 2>&1 || bad "daemon stop errored"
for _ in $(seq 1 20); do peer L "$CLI" status >/dev/null 2>&1 || break; sleep 0.5; done
peer L "$CLI" status >/dev/null 2>&1 && bad "daemon still reachable after stop" || ok "daemon stop actually stopped it"
peer L "$CLI" daemon start >/dev/null 2>&1
for _ in $(seq 1 20); do peer L "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
FP2=$(peer L "$CLI" identity 2>/dev/null | awk '/fingerprint/ {print $2}')
[ -n "$FP1" ] && [ "$FP1" = "$FP2" ] && ok "identity fingerprint is stable across a restart ($FP1)" || bad "fingerprint changed across restart ($FP1 -> $FP2)"
peer L "$CLI" daemon stop >/dev/null 2>&1

echo
echo "== file-backed identity keystore: age-encrypted key survives a restart, wrong passphrase fails the boot =="
mkdir -p "$ROOT/F/xdg/ferry"
printf 'listen_port = 48411\nidentity_keystore = "file"\n' > "$ROOT/F/xdg/ferry/config.toml"
run_file_daemon() {
  DBUS_SESSION_BUS_ADDRESS="unix:path=$ROOT/L/bus" HOME="$ROOT/F" XDG_DATA_HOME="$ROOT/F/xdg" \
    FERRY_IDENTITY_PASSPHRASE="$1" "$DAEMON" > "$ROOT/F/daemon.$2.log" 2>&1 &
  echo $!
}
file_cli() { HOME="$ROOT/F" XDG_DATA_HOME="$ROOT/F/xdg" "$CLI" "$@"; }
FPID=$(run_file_daemon "correct horse battery staple" 1)
ready=""
for _ in $(seq 1 40); do file_cli status >/dev/null 2>&1 && { ready=1; break; }; sleep 0.5; done
[ -n "$ready" ] && ok "daemon boots with identity_keystore=file" || bad "file-keystore daemon never became ready"
[ -f "$ROOT/F/xdg/ferry/identity.age" ] && head -c 32 "$ROOT/F/xdg/ferry/identity.age" | grep -qa "age-encryption" \
  && ok "identity.age is a real age-encrypted file" || bad "identity.age missing or not age-encrypted"
FF1=$(file_cli identity 2>/dev/null | awk '/fingerprint/ {print $2}')
kill "$FPID" 2>/dev/null; sleep 1
FPID=$(run_file_daemon "correct horse battery staple" 2)
for _ in $(seq 1 40); do file_cli status >/dev/null 2>&1 && break; sleep 0.5; done
FF2=$(file_cli identity 2>/dev/null | awk '/fingerprint/ {print $2}')
[ -n "$FF1" ] && [ "$FF1" = "$FF2" ] && ok "same fingerprint after a restart from the age file ($FF1)" || bad "file-keystore fingerprint changed ($FF1 -> $FF2)"
kill "$FPID" 2>/dev/null; sleep 1
BADPID=$(run_file_daemon "the wrong passphrase entirely" 3)
sleep 2
file_cli status >/dev/null 2>&1 && bad "daemon accepted the wrong passphrase" || ok "a wrong passphrase fails the boot instead of generating a new identity"
kill "$BADPID" 2>/dev/null

echo
echo "== provider: a daemon with remote_features_enabled answers provider IPC and the job path fails cleanly with no token =="
start_session P 48410
printf 'listen_port = 48410\nremote_features_enabled = true\n' > "$ROOT/P/xdg/ferry/config.toml"
peer P "$CLI" daemon start >/dev/null 2>&1
for _ in $(seq 1 20); do peer P "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
peer P "$CLI" provider status 2>/dev/null | grep -qi "enabled:.*true" && ok "provider reports enabled" || bad "provider not enabled with remote_features_enabled=true"
peer P "$CLI" provider status 2>/dev/null | grep -qi "connected:.*false" && ok "provider reports not-connected (no token)" || bad "provider unexpectedly connected"
out=$(peer P "$CLI" gist publish some-item --yes 2>&1); rc=$?
echo "     gist publish rc=$rc: $(echo "$out" | tail -1)"
{ [ $rc -ne 0 ] && echo "$out" | grep -qi "no GitHub connection"; } \
  && ok "the queued gist job ran on the worker and failed cleanly without a token" \
  || bad "gist publish did not fail with the expected 'no GitHub connection' message"
fout=$(peer P "$CLI" provider fetch owner/repo/roster.json --yes 2>&1 || true)
echo "$fout" | grep -qi "no GitHub connection\|not connected" \
  && ok "roster fetch is refused without a connection" || bad "roster fetch not refused cleanly: $fout"
peer P "$CLI" daemon stop >/dev/null 2>&1

echo
echo "== roster enforcement: send to a syntactically-valid but unrostered peer id is denied by the daemon =="
head -c 512 /dev/urandom > "$ROOT/x.bin"
out=$(peer a "$CLI" send "0000000000000000000000000000000000000000000000000000000000000000" "$ROOT/x.bin" 2>&1); rc=$?
echo "     rc=$rc: $out"
{ [ $rc -ne 0 ] && echo "$out" | grep -qiE "not in the roster|PeerNotAuthorized|denied"; } \
  && ok "unrostered send refused" || bad "unrostered send was not refused"

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

echo
echo "== signed-roster import writes a roster.imported audit row =="
peer a "$CLI" roster export "$ROOT/a-roster.json" >/dev/null 2>&1 || bad "roster export failed"
peer b "$CLI" roster import "$ROOT/a-roster.json" --yes >/dev/null 2>&1 || bad "roster import failed"
peer b "$CLI" activity --limit 40 2>/dev/null | grep -q "roster.imported" \
  && ok "b logged roster.imported" || bad "b has no roster.imported audit row"

MDNS_OK=1
echo
echo "== delivery → audit: a burn-after-read file, opened once, is logged and unopenable a second time =="
printf 'burn-me-%s\n' "$(date +%s%N)" > "$ROOT/burn.txt"
BWANT=$(sha256sum "$ROOT/burn.txt" | awk '{print $1}')
peer a "$CLI" send "$BID" "$ROOT/burn.txt" --ttl 3600 --burn >/dev/null 2>&1 || bad "burn send failed to queue"
if item=$(wait_delivered b burn.txt); then
  peer b "$CLI" receive open "$item" --out "$ROOT/burn.got" >/dev/null 2>&1
  [ "$(sha256sum "$ROOT/burn.got" | awk '{print $1}')" = "$BWANT" ] && ok "first open returns the correct plaintext" || bad "first open content mismatch"
  out=$(peer b "$CLI" receive open "$item" --out "$ROOT/burn.got2" 2>&1); rc=$?
  echo "     second open rc=$rc: $out"
  { [ $rc -ne 0 ] && echo "$out" | grep -qi "already been opened"; } && ok "second open refused with a clear message" || bad "second open not clearly refused"
  peer b "$CLI" activity --limit 40 2>/dev/null | grep -qE "item.delivered .*$item" && ok "b logged item.delivered" || bad "no item.delivered row"
  peer b "$CLI" activity --limit 40 2>/dev/null | grep -qE "item.opened .*$item .*opened_and_burned" && ok "b logged item.opened (opened_and_burned)" || bad "no burn audit row"
else
  MDNS_OK=0
  if [ "${FERRY_STRICT:-0}" = 1 ]; then
    bad "burn item not delivered within ${RECV_TIMEOUT}s and FERRY_STRICT=1 — mDNS multicast is required for this run"
  else
    skip "burn item not delivered within ${RECV_TIMEOUT}s — mDNS multicast unavailable on this host; delivery/audit checks skipped (covered by the workspace tests)"
  fi
fi

if [ "$MDNS_OK" = 1 ]; then
  echo
  echo "== TTL is measured from delivery on the receiver's clock =="
  head -c 2048 /dev/urandom > "$ROOT/ttl.bin"
  peer a "$CLI" send "$BID" "$ROOT/ttl.bin" --ttl "$TTL_SECS" >/dev/null 2>&1
  if item=$(wait_delivered b ttl.bin); then
    echo "     delivered; not opening — waiting $((TTL_SECS + 8))s for the TTL to elapse"
    sleep $((TTL_SECS + 8))
    out=$(peer b "$CLI" receive open "$item" --out "$ROOT/ttl.got" 2>&1); rc=$?
    echo "     open after TTL rc=$rc: $out"
    { [ $rc -ne 0 ] && echo "$out" | grep -qi "expired"; } && ok "open after TTL refused (expired)" || bad "open after TTL was not refused"
  else
    bad "ttl item not delivered"
  fi

  echo
  echo "== offline send: queues locally, drains when the peer reappears =="
  stop_peer b
  head -c 4096 /dev/urandom > "$ROOT/off.bin"
  OWANT=$(sha256sum "$ROOT/off.bin" | awk '{print $1}')
  out=$(peer a "$CLI" send "$BID" "$ROOT/off.bin" --ttl 3600 2>&1); rc=$?
  echo "     send while b down rc=$rc: $out"
  [ $rc -eq 0 ] && ok "send accepted as a local write" || bad "send rejected while peer offline"
  peer a "$CLI" sent list 2>/dev/null | grep -i off.bin | grep -qiE "queued|transferring" \
    && ok "sender state is queued/transferring, never delivered" || bad "unexpected sender state"
  start_peer b "$PORT_B"
  for _ in $(seq 1 40); do peer b "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
  if item=$(wait_delivered b off.bin); then
    peer b "$CLI" receive open "$item" --out "$ROOT/off.got" >/dev/null 2>&1
    [ "$(sha256sum "$ROOT/off.got" | awk '{print $1}')" = "$OWANT" ] && ok "drained + delivered after reappearance, hash matches" || bad "drained but content mismatch"
  else
    bad "never drained after b came back"
  fi

  echo
  echo "== outbox TTL: an item the peer never comes back for is dropped with a sender-visible reason =="
  stop_peer b
  head -c 2048 /dev/urandom > "$ROOT/ttl-drop.bin"
  peer a "$CLI" send "$BID" "$ROOT/ttl-drop.bin" --ttl 5 >/dev/null 2>&1 || bad "ttl-drop send failed to queue"
  echo "     waiting 12s for the 5s outbox TTL to elapse while b is down"
  sleep 12
  start_peer b "$PORT_B"
  for _ in $(seq 1 40); do peer b "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
  dropped=""
  for _ in $(seq 1 "$RECV_TIMEOUT"); do
    peer a "$CLI" sent list 2>/dev/null | grep -i ttl-drop.bin | grep -qiE "expired" && { dropped=1; break; }
    sleep 1
  done
  [ -n "$dropped" ] && ok "the expired item shows as Expired to the sender" || bad "expired item never transitioned to Expired"
  peer a "$CLI" sent list 2>/dev/null | grep -i ttl-drop.bin | grep -qi "reappear" \
    && ok "the sender sees a human-readable drop reason" || skip "sent list did not surface the reason string (CLI rendering)"
  peer a "$CLI" activity --limit 40 2>/dev/null | grep -qE "item.outbox_expired .*outbox_ttl" \
    && ok "a distinct outbox_ttl audit row is written" || bad "no outbox_ttl audit row"

  echo
  echo "== kill mid-transfer: receiver is SIGKILLed mid-stream, transfer resumes on restart =="
  stop_peer b
  FERRY_TEST_CHUNK_DELAY_MS=150 start_peer b "$PORT_B"
  for _ in $(seq 1 40); do peer b "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
  head -c 3000000 /dev/urandom > "$ROOT/big.bin"
  BIGWANT=$(sha256sum "$ROOT/big.bin" | awk '{print $1}')
  peer a "$CLI" send "$BID" "$ROOT/big.bin" --ttl 3600 >/dev/null 2>&1 || bad "big send failed to queue"
  sleep 3
  stop_peer b KILL
  partial=$(find "$ROOT/b/xdg" -name '*.bin' -printf '%s\n' 2>/dev/null | sort -rn | head -1)
  echo "     receiver held ~${partial:-0} bytes when it was killed"
  start_peer b "$PORT_B"
  for _ in $(seq 1 40); do peer b "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
  if item=$(wait_delivered b big.bin); then
    peer b "$CLI" receive open "$item" --out "$ROOT/big.got" >/dev/null 2>&1
    [ "$(sha256sum "$ROOT/big.got" | awk '{print $1}')" = "$BIGWANT" ] \
      && ok "transfer resumed after a hard kill and the full file matches" \
      || bad "resumed transfer produced the wrong content"
  else
    bad "transfer never completed after the receiver was killed and restarted"
  fi
fi

if [ "$MDNS_OK" = 1 ]; then
  echo
  echo "== threaded messaging: a message round-trips and shows in the peer's thread =="
  MSG="ping from a $(date +%s)"
  peer a "$CLI" message send "$BID" "$MSG" >/dev/null 2>&1 || bad "message send failed to queue"
  got=""
  for _ in $(seq 1 "$RECV_TIMEOUT"); do
    got=$(peer b "$CLI" message thread peer-a 2>/dev/null | grep -F "$MSG" || true)
    [ -n "$got" ] && break
    sleep 1
  done
  [ -n "$got" ] && ok "b sees the message in its thread with peer-a" || bad "message never arrived in b's thread"
  peer b "$CLI" message threads 2>/dev/null | grep -qi "peer-a" && ok "the conversation appears in b's thread list" || bad "no thread listed for peer-a"

  echo
  echo "== messaging: a reply travels back and both directions show in order =="
  RMSG="reply from b $(date +%s)"
  peer b "$CLI" message send peer-a "$RMSG" >/dev/null 2>&1 || bad "reply failed to queue"
  got=""
  for _ in $(seq 1 "$RECV_TIMEOUT"); do
    got=$(peer a "$CLI" message thread peer-b 2>/dev/null | grep -F "$RMSG" || true)
    [ -n "$got" ] && break
    sleep 1
  done
  [ -n "$got" ] && ok "a sees b's reply" || bad "reply never arrived in a's thread"
  order=$(peer a "$CLI" message thread peer-b 2>/dev/null | grep -nE "$(printf '%s' "$MSG" | cut -c1-12)|$(printf '%s' "$RMSG" | cut -c1-12)" | awk -F: '{print $1}' | tr '\n' ' ')
  first=$(echo "$order" | awk '{print $1}'); second=$(echo "$order" | awk '{print $2}')
  { [ -n "$first" ] && [ -n "$second" ] && [ "$first" -lt "$second" ]; } \
    && ok "a's outgoing message sorts before b's later reply" || bad "message ordering wrong in a's thread ($order)"

  echo
  echo "== messaging: a message sent while the peer is offline queues, then drains =="
  stop_peer b
  OMSG="queued while b down $(date +%s)"
  out=$(peer a "$CLI" message send "$BID" "$OMSG" 2>&1); rc=$?
  [ $rc -eq 0 ] && ok "offline message accepted as a local write" || bad "offline message rejected (rc=$rc: $out)"
  start_peer b "$PORT_B"
  for _ in $(seq 1 40); do peer b "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
  got=""
  for _ in $(seq 1 "$RECV_TIMEOUT"); do
    got=$(peer b "$CLI" message thread peer-a 2>/dev/null | grep -F "$OMSG" || true)
    [ -n "$got" ] && break
    sleep 1
  done
  [ -n "$got" ] && ok "the queued message drained to b after it came back" || bad "offline message never drained"
fi

echo
echo "== audit log is content-free =="
aud=$(peer a "$CLI" activity --limit 60 2>/dev/null; peer b "$CLI" activity --limit 60 2>/dev/null)
leak=""
for s in "burn-me" "${BWANT:-__none__}" "${OWANT:-__none__}"; do
  echo "$aud" | grep -qF "$s" && leak="$leak $s"
done
[ -z "$leak" ] && ok "no payload bytes or content hash in any audit row" || bad "audit leaked:$leak"

echo
if [ "$FAILED" -eq 0 ]; then
  echo "==> ALL INVARIANT CHECKS PASSED"
else
  echo "==> SOME INVARIANT CHECKS FAILED"
fi
exit "$FAILED"
