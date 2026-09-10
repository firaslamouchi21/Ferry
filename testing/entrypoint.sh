#!/bin/sh
set -e

export HOME="${HOME:-/root}"
mkdir -p /data/ferry

if [ -n "${FERRY_IDENTITY_PASSPHRASE:-}" ]; then
    if [ ! -f /data/ferry/config.toml ]; then
        {
            [ -n "${FERRY_LISTEN_PORT:-}" ] && printf 'listen_port = %s\n' "$FERRY_LISTEN_PORT"
            printf 'identity_keystore = "file"\n'
            [ -n "${FERRY_REMOTE_FEATURES:-}" ] && printf 'remote_features_enabled = %s\n' "$FERRY_REMOTE_FEATURES"
        } > /data/ferry/config.toml
    fi
    exec "$@"
fi

export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=/run/ferry-session-bus}"
mkdir -p "$HOME/.local/share/keyrings"

secrets_up() {
    dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply \
        /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null \
        | grep -q "boolean true"
}

if ! secrets_up; then
    rm -f "$(printf '%s' "$DBUS_SESSION_BUS_ADDRESS" | sed 's/^unix:path=//')" 2>/dev/null || true
    dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --nofork --nopidfile &
    for _ in $(seq 1 20); do
        [ -S "$(printf '%s' "$DBUS_SESSION_BUS_ADDRESS" | sed 's/^unix:path=//')" ] && break
        sleep 0.25
    done

    rm -f "$HOME"/.local/share/keyrings/* 2>/dev/null || true
    printf '\n' | gnome-keyring-daemon --unlock --components=secrets,pkcs11 >/dev/null 2>&1 &

    for _ in $(seq 1 40); do
        secrets_up && break
        sleep 0.5
    done
fi

exec "$@"
