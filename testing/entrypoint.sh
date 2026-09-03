#!/bin/sh
set -e

export HOME="${HOME:-/root}"
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=/run/ferry-session-bus}"
mkdir -p /data "$HOME/.local/share/keyrings"

if [ ! -S "$(printf '%s' "$DBUS_SESSION_BUS_ADDRESS" | sed 's/^unix:path=//')" ]; then
    dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --nofork --nopidfile &
    for _ in $(seq 1 20); do
        [ -S "$(printf '%s' "$DBUS_SESSION_BUS_ADDRESS" | sed 's/^unix:path=//')" ] && break
        sleep 0.25
    done

    rm -f "$HOME"/.local/share/keyrings/* 2>/dev/null || true
    printf '\n' | gnome-keyring-daemon --unlock --components=secrets,pkcs11 >/dev/null 2>&1 &

    for _ in $(seq 1 40); do
        if dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply \
            /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null \
            | grep -q "boolean true"; then
            break
        fi
        sleep 0.5
    done
fi

exec "$@"
