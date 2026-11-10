#!/bin/bash
# Fails unless a built Conch.app is signed with the stable identity whose SHA-1
# hash is given.
#
# macOS keys the Accessibility grant to the app's designated requirement. An
# ad-hoc signature makes that requirement a hash of the binary, so every
# rebuild or update loses the grant. The stable identity gives every build the
# requirement
#   identifier "app.conch.desktop" and certificate leaf = H"<identity hash>"
#
# Usage: scripts/verify-signing.sh <Conch.app> <identity-sha1>
set -euo pipefail

app=${1:?usage: verify-signing.sh <Conch.app> <identity-sha1>}
identity=${2:?usage: verify-signing.sh <Conch.app> <identity-sha1>}

requirement=$(codesign -d -r- "$app" 2>&1 | sed -n 's/^designated => //p')
expected="identifier \"app.conch.desktop\" and certificate leaf = H\"$(echo "$identity" | tr 'A-F' 'a-f')\""
if [ "$requirement" != "$expected" ]; then
  echo "error: $app has designated requirement:" >&2
  echo "  $requirement" >&2
  echo "expected:" >&2
  echo "  $expected" >&2
  echo "The Accessibility grant would not survive this build." >&2
  exit 1
fi
codesign --verify --deep --strict "$app"
echo "signed with stable requirement: $requirement"
