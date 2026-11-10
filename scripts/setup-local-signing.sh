#!/bin/bash
# Creates a self-signed code-signing identity in the login keychain so locally
# built Conch bundles keep one code identity across rebuilds. Ad-hoc signing
# (identity "-") makes the designated requirement a hash of the binary, so macOS
# invalidates the Accessibility grant on every build.
#
# Usage: scripts/setup-local-signing.sh
# Then:  APPLE_SIGNING_IDENTITY=<printed hash> npm run tauri -- build
# Remove: security delete-identity -Z <printed hash> ~/Library/Keychains/login.keychain-db
set -euo pipefail

NAME="Conch Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

existing=$(security find-certificate -c "$NAME" -Z "$KEYCHAIN" 2>/dev/null | awk '/SHA-1 hash:/ {print $3; exit}' || true)
if [ -n "$existing" ]; then
  echo "$existing"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
password=$(uuidgen)

cat >"$tmp/openssl.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem" -config "$tmp/openssl.cnf" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
  -out "$tmp/identity.p12" -passout "pass:$password" 2>/dev/null
security import "$tmp/identity.p12" -k "$KEYCHAIN" -P "$password" -T /usr/bin/codesign >/dev/null

# Let codesign use the private key without a GUI prompt on every build.
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "" "$KEYCHAIN" >/dev/null 2>&1 || true

security find-certificate -c "$NAME" -Z "$KEYCHAIN" | awk '/SHA-1 hash:/ {print $3; exit}'
