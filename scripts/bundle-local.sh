#!/bin/bash
# Builds Conch.app signed with the persistent "Conch Local Signing" identity and
# refuses to finish if the bundle ended up ad-hoc signed.
#
# macOS keys the Accessibility grant to the app's designated requirement. An
# ad-hoc signature makes that requirement a hash of the binary, so every
# rebuild loses the grant. The local identity gives every build the requirement
#   identifier "app.conch.desktop" and certificate leaf = H"<identity hash>"
#
# Usage: scripts/bundle-local.sh [--debug] [--install]
#   --debug    build the debug profile (smaller and faster to compile)
#   --install  quit a running Conch and replace /Applications/Conch.app
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
install=0
extra=()
for arg in "$@"; do
  case "$arg" in
    --install) install=1 ;;
    --debug) profile=debug; extra+=(--debug) ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

# tauri-build bundles whatever matches workers/*, so a missing runtime build
# would still package an app that cannot transcribe. Stop before building.
workers=(whisper-server llama-server)
for name in "${workers[@]}"; do
  if [ ! -x "src-tauri/workers/$name" ]; then
    echo "error: src-tauri/workers/$name is missing; run python3 scripts/build-runtimes.py first" >&2
    exit 1
  fi
done

identity=$(scripts/setup-local-signing.sh)
app="src-tauri/target/$profile/bundle/macos/Conch.app"

# The command-line config overrides the "-" (ad-hoc) default in tauri.conf.json.
npm run tauri -- build --bundles app ${extra[@]+"${extra[@]}"} \
  --config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$identity\"}}}"

for name in "${workers[@]}"; do
  if [ ! -x "$app/Contents/Resources/workers/$name" ]; then
    echo "error: $app does not contain workers/$name" >&2
    exit 1
  fi
done

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

if [ "$install" = 1 ]; then
  osascript -e 'tell application id "app.conch.desktop" to quit' 2>/dev/null || true
  while pgrep -x conch >/dev/null; do sleep 0.2; done
  rm -rf /Applications/Conch.app
  ditto "$app" /Applications/Conch.app
  echo "installed /Applications/Conch.app"
fi
