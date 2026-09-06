#!/usr/bin/env bash
set -euo pipefail

dir="${1:?usage: sign-linux.sh <dir>}"

if [ -z "${GPG_PRIVATE_KEY:-}" ]; then
  echo "sign-linux: GPG_PRIVATE_KEY not set — skipping detached signatures"
  exit 0
fi

keyring="$(mktemp -d)"
trap 'rm -rf "$keyring"' EXIT
export GNUPGHOME="$keyring"
chmod 700 "$GNUPGHOME"

echo "$GPG_PRIVATE_KEY" | gpg --batch --import

for f in "$dir"/ferry*; do
  case "$f" in
    *.asc | *.sig | *SHA256SUMS*) continue ;;
  esac
  gpg --batch --yes --pinentry-mode loopback \
      --passphrase "${GPG_PASSPHRASE:-}" \
      --armor --detach-sign --output "$f.asc" "$f"
  echo "signed $f -> $f.asc"
done
