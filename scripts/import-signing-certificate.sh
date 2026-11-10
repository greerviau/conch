#!/bin/bash
# Imports the stable signing certificate into a temporary keychain on a CI
# runner and adds it to the keychain search list, so `codesign` can sign with
# the identity hash.
#
# Tauri can import a certificate itself when APPLE_CERTIFICATE is set, but then
# it only accepts certificates named like Apple's ("Developer ID Application:").
# The self-signed "Conch Local Signing" certificate is not, so Tauri finds no
# identity and fails. Signing by hash, as scripts/bundle-local.sh does, works.
#
# Usage: APPLE_CERTIFICATE=<base64 .p12> APPLE_CERTIFICATE_PASSWORD=<password> \
#          scripts/import-signing-certificate.sh
# Prints the keychain path.
set -euo pipefail

: "${APPLE_CERTIFICATE:?set APPLE_CERTIFICATE to the base64 of the .p12}"
: "${APPLE_CERTIFICATE_PASSWORD:?set APPLE_CERTIFICATE_PASSWORD}"

dir=${RUNNER_TEMP:-$(mktemp -d)}
keychain="$dir/conch-signing.keychain-db"
p12="$dir/conch-signing.p12"
password=$(uuidgen)

printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$p12"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
# Let codesign use the private key without a GUI prompt.
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
rm -f "$p12"

keychains=("$keychain")
while IFS= read -r line; do
  line=${line#"${line%%[![:space:]]*}"}
  line=${line%\"}
  line=${line#\"}
  [ -n "$line" ] && keychains+=("$line")
done < <(security list-keychains -d user)
security list-keychains -d user -s "${keychains[@]}"
echo "$keychain"
