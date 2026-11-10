<img src="assets/icon.svg" width="64" height="64" alt="">

# Conch

Local, repository-aware dictation for writing agent prompts.
Press a global shortcut, speak, and Conch pastes the cleaned transcript into the focused text field, or leaves it on the clipboard.
It runs on macOS, Windows, and Linux with Tauri 2, React, and Rust.
Core dictation needs no account and works offline after a model download.

## Quick start

Install Node 24 or later, Rust 1.95.0, CMake, Git, a C++ toolchain, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
python3 scripts/build-runtimes.py   # use python on Windows
npm run tauri -- dev
```

In the app, download the recognition model, enable microphone and paste access, choose a repository, then dictate with Command-or-Control + Shift + Space.
On macOS, install local builds with `npm run bundle:local -- --install` so the Accessibility grant survives rebuilds.

## Docs

- [Using Conch](docs/usage.md): setup, recording, background mode, and delivery
- [Permissions and platform status](docs/permissions.md)
- [Cleanup and privacy](docs/privacy.md)
- [Development, packaging, and branding](docs/development.md)
- [Validation evidence](docs/validation/README.md) and [latency measurements](docs/validation/latency.md)
- [Spec](docs/plans/2026-11-09-conch.md) and [glossary](docs/UBIQUITOUS-LANGUAGE.md)
- [AGENTS.md](AGENTS.md): module layout, conventions, and pitfalls for coding agents

## License

Conch is released under the [MIT license](LICENSE).
The bundled inference workers and the S1-mini model keep their own licenses, which ship with the app.
