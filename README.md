<img src="assets/icon.svg" width="64" height="64" alt="">

# Conch

Simple, local, context-aware dictation.

Press a global shortcut, speak, and Conch pastes the cleaned transcript into the focused text field. Speech recognition runs on your machine, needs no account, and works offline after a model download.

Conch runs on macOS, Windows, and Linux.

## Install

macOS (Apple Silicon) and Linux (x86_64):

```sh
curl -fsSL https://raw.githubusercontent.com/greerviau/conch/main/scripts/install.sh | sh
```

Windows, and other formats (`.dmg`, `.deb`, `.rpm`): download from the [latest release](https://github.com/greerviau/conch/releases/latest).

On first launch, choose a recognition model, then grant microphone and paste access. Dictate with Command/Control + Shift + Space.

## Build from source

Install Node 24 or later, Rust 1.95.0, CMake, Git, a C++ toolchain, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
python3 scripts/build-runtimes.py   # use python on Windows
npm run tauri -- dev
```

## Documentation

- [Using Conch](docs/usage.md)
- [Permissions and platform status](docs/permissions.md)
- [Cleanup and privacy](docs/privacy.md)
- [Development, packaging, and branding](docs/development.md)
- [Spec](docs/plans/2026-11-09-conch.md) and [glossary](docs/UBIQUITOUS-LANGUAGE.md)
- [AGENTS.md](AGENTS.md)

## License

[MIT](LICENSE). The bundled inference workers and models keep their own licenses, which ship with the app.
