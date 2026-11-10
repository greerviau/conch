#!/bin/sh
# Installs the latest Conch release.
#
#   curl -fsSL https://raw.githubusercontent.com/greerviau/conch/main/scripts/install.sh | sh
#
# macOS (Apple Silicon): puts Conch.app in /Applications, or ~/Applications when
# /Applications is not writable. curl does not set the quarantine flag, so
# Gatekeeper does not block the first launch of this unnotarized app.
# Linux (x86_64): saves the AppImage to ~/.local/bin/Conch.AppImage. An
# AppImage updates itself from inside Conch.
# Windows: download the installer from the releases page.
#
# CONCH_OS, CONCH_ARCH, CONCH_DOWNLOAD_BASE, CONCH_API_URL, CONCH_INSTALL_DIR,
# and CONCH_BIN_DIR override the system, the sources, and the destinations.
set -eu

repo=greerviau/conch
releases="https://github.com/$repo/releases/latest"
base=${CONCH_DOWNLOAD_BASE:-$releases/download}
api=${CONCH_API_URL:-https://api.github.com/repos/$repo/releases/latest}
os=${CONCH_OS:-$(uname -s)}
arch=${CONCH_ARCH:-$(uname -m)}

fail() {
  echo "conch install: $*" >&2
  exit 1
}

install_macos() {
  dest=${CONCH_INSTALL_DIR:-/Applications}
  if [ ! -w "$dest" ]; then
    dest=$HOME/Applications
  fi
  mkdir -p "$dest"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT

  curl -fsSL "$base/Conch_aarch64.app.tar.gz" -o "$tmp/conch.tar.gz" ||
    fail "Could not download Conch from $base. Get the installer from $releases"
  tar -xzf "$tmp/conch.tar.gz" -C "$tmp" || fail "The download is not a valid archive"
  [ -d "$tmp/Conch.app" ] || fail "The download does not contain Conch.app"
  codesign --verify --deep --strict "$tmp/Conch.app" 2>/dev/null ||
    fail "The code signature of the download does not verify, so nothing was installed"
  if pgrep -f "$dest/Conch.app/Contents/MacOS/" >/dev/null 2>&1; then
    fail "Conch is running. Quit it, then run this again"
  fi

  rm -rf "${dest:?}/Conch.app"
  mv "$tmp/Conch.app" "$dest/Conch.app"
  echo "Installed $dest/Conch.app"
}

install_linux() {
  bin=${CONCH_BIN_DIR:-$HOME/.local/bin}
  url=$(curl -fsSL "$api" |
    grep -o '"browser_download_url": *"[^"]*_amd64\.AppImage"' |
    head -n 1 |
    sed 's/.*: *"//; s/"$//') || true
  [ -n "${url:-}" ] || fail "The latest release has no AppImage. Get an installer from $releases"

  mkdir -p "$bin"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  curl -fsSL "$url" -o "$tmp/Conch.AppImage" || fail "Could not download $url"
  chmod +x "$tmp/Conch.AppImage"
  mv "$tmp/Conch.AppImage" "$bin/Conch.AppImage"
  echo "Installed $bin/Conch.AppImage"
}

case "$os/$arch" in
  Darwin/arm64) install_macos ;;
  Darwin/*) fail "The macOS build is for Apple Silicon only. Build from source: https://github.com/$repo#build-from-source" ;;
  Linux/x86_64) install_linux ;;
  *) fail "There is no installer script for $os $arch. Download one from $releases" ;;
esac
