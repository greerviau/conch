<img src="assets/icon.svg" width="64" height="64" alt="">

# Conch

Local, repository-aware dictation for writing agent prompts.
Press a global shortcut, speak, and Conch pastes the cleaned transcript into the focused text field, or leaves it on the clipboard.
It runs on macOS, Windows, and Linux with Tauri 2, React, and Rust.
Core dictation needs no account and works offline after a model download.

## Install

On macOS (Apple Silicon) or Linux (x86_64), run:

```sh
curl -fsSL https://raw.githubusercontent.com/greerviau/conch/main/scripts/install.sh | sh
```

The script is short and worth reading first.
It downloads the latest [release](https://github.com/greerviau/conch/releases/latest) from this repository over HTTPS.
On macOS it checks that the app's code signature is intact and installs `Conch.app` into `/Applications`, or `~/Applications` when that is not writable.
`curl` does not set the quarantine flag, so Gatekeeper does not block the first launch of this unnotarized app.
On Linux it saves the AppImage to `~/.local/bin/Conch.AppImage`.

On Windows, download the `.exe` or `.msi` from the [releases page](https://github.com/greerviau/conch/releases/latest).
SmartScreen warns about it because the installer is not signed with a paid certificate; choose More info, then Run anyway.

Every release also attaches a `.dmg` for macOS and `.deb` and `.rpm` packages for Linux.
Gatekeeper blocks a `.dmg` downloaded in a browser, because Conch is not notarized.
To open it, go to System Settings, then Privacy & Security, and choose Open Anyway.
The macOS build is for Apple Silicon only, so on an Intel Mac [build from source](#build-from-source).

After installing, open Conch, choose and download a recognition model, and enable microphone and paste access.
Conch checks GitHub for newer releases and installs one when you click Install; Settings turns the check off.

Only the macOS install has been tested.
The Linux script and the Windows and Linux installers have not been run on those systems.

## Build from source

Install Node 24 or later, Rust 1.95.0, CMake, Git, a C++ toolchain, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
python3 scripts/build-runtimes.py   # use python on Windows
npm run tauri -- dev
```

In the app, choose and download a recognition model, enable microphone and paste access, choose a repository, then dictate with Command-or-Control + Shift + Space.
On macOS, install local builds with `npm run bundle:local -- --install` so the Accessibility grant survives rebuilds.

## Docs

- [Using Conch](docs/usage.md): setup, recording, background mode, updates, and delivery
- [Permissions and platform status](docs/permissions.md)
- [Cleanup and privacy](docs/privacy.md)
- [Development, packaging, and branding](docs/development.md)
- [Validation evidence](docs/validation/README.md) and [latency measurements](docs/validation/latency.md)
- [Spec](docs/plans/2026-11-09-conch.md) and [glossary](docs/UBIQUITOUS-LANGUAGE.md)
- [AGENTS.md](AGENTS.md): module layout, conventions, and pitfalls for coding agents

## License

Conch is released under the [MIT license](LICENSE).
The bundled inference workers and the S1-mini model keep their own licenses, which ship with the app.
