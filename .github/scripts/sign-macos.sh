#!/usr/bin/env bash
set -euo pipefail

dir="${1:?usage: sign-macos.sh <dir>}"

if [ -z "${MACOS_CERT_P12:-}" ]; then
  echo "sign-macos: MACOS_CERT_P12 not set — skipping codesign/notarize"
  exit 0
fi

keychain="$RUNNER_TEMP/ferry-signing.keychain-db"
keychain_pw="$(openssl rand -base64 24)"
security create-keychain -p "$keychain_pw" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_pw" "$keychain"

cert="$RUNNER_TEMP/cert.p12"
echo "$MACOS_CERT_P12" | base64 --decode > "$cert"
security import "$cert" -k "$keychain" -P "${MACOS_CERT_PASSWORD:-}" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_pw" "$keychain"
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | sed s/\"//g)

identity="$(security find-identity -v -p codesigning "$keychain" | awk 'NR==1{print $2}')"

for f in "$dir"/ferry*; do
  case "$f" in *SHA256SUMS*) continue ;; esac
  codesign --force --options runtime --timestamp --sign "$identity" "$f"
  codesign --verify --strict --verbose=2 "$f"
done

if [ -n "${MACOS_NOTARY_KEY:-}" ]; then
  key="$RUNNER_TEMP/notary.p8"
  echo "$MACOS_NOTARY_KEY" | base64 --decode > "$key"
  for f in "$dir"/ferry*; do
    case "$f" in *SHA256SUMS*) continue ;; esac
    zip="$f.zip"
    ditto -c -k "$f" "$zip"
    xcrun notarytool submit "$zip" \
      --key "$key" --key-id "$MACOS_NOTARY_KEY_ID" --issuer "$MACOS_NOTARY_ISSUER" \
      --wait
    rm -f "$zip"
  done
fi

security delete-keychain "$keychain"
