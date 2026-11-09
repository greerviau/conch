<img src="assets/icon.svg" width="64" height="64" alt="">

# Conch

Conch prepares dictated prompts using selected local repositories and exact phrase mappings.
It uses Tauri 2, React, TypeScript, and Rust.
Core dictation requires no account or payment and works offline after an explicit recognition-model download.
Basic cleanup is the default.

## Run locally

Install Node 24 or later, Rust 1.95.0, CMake, Git, a C++ toolchain, and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/).
Build the pinned inference workers before starting Conch.

```sh
npm ci
python3 scripts/build-runtimes.py
npm run tauri -- dev
```

On Windows, use `python` in place of `python3` in these commands.
The Dictation page shows the setup checklist and a Download recognition model button.
Download Whisper tiny.en there for English recognition.
Enable microphone access and automatic paste from the same checklist.
Conch reports permission status when you return from System Settings.
The optional S1-mini by Superwhisper download enables managed Enhanced cleanup.
Conch verifies model size and SHA-256 before installation.
Model files remain in Conch's local application data directory.

Choose a repository folder, save, and refresh its index.
Manual path entry remains available under Enter a path manually.
Select the repositories that supply context before recording.
Define global or repository mappings for recurring transcription errors.
The default shortcut is Command-or-Control + Shift + Space in push-to-talk mode.
In Settings, click Change shortcut and press the desired combination.
Conch pauses its registered shortcut during capture and ignores repeat events.
Escape or leaving the control cancels capture.
Click Save changes to register the new shortcut.
The in-app recording buttons work independently of shortcut availability.
Recordings stop after 180 seconds.
A floating indicator shows microphone startup, recording level, duration, processing, and the delivery outcome above other apps.
The indicator ignores pointer input and does not activate Conch.
Settings can disable the indicator.
Minimize Conch to leave it available while dictating in other apps.
Closing its main window quits Conch and cancels active work.

Conch copies each initial nonempty result once.
Conch requests one paste when the same eligible text input remains focused across the delivery check.
It never sends Enter.
Uncertain or secure fields receive clipboard fallback.
Review edits and accepted suggestions require explicit copying.
Keep original rejects a proposed edit, and Undo reverses an accepted edit when its replacement remains uniquely identifiable after manual editing.
The raw transcript remains available until another result replaces it or Conch closes.

## Permissions and platform status

macOS requests microphone access on the first recording.
Click Enable automatic paste in setup to open System Settings / Privacy & Security / Accessibility.
Enable Conch there for focused-input detection and paste.
Ad-hoc signed local updates can require removing and re-adding Conch in that list.
Missing Accessibility permission keeps clipboard delivery available.
Conch accepts enabled AXTextField, AXTextArea, and AXComboBox controls when Accessibility exposes a writable value or selected-text setter.
It excludes secure fields and its own process.
Clipboard fallback reports the reason when permission, editability, or stable focus is missing.
Windows uses UI Automation Edit controls with a writable Value pattern and excludes password fields.
Linux uses clipboard fallback for automatic delivery.
Global shortcuts depend on the desktop session.
Conch disables them in detected Wayland sessions and retains the in-app controls.

The current validation environment is an Apple M1 Mac with 8 GiB RAM on macOS 26.6.2.
Windows builds, Linux builds, X11, Wayland, target-app paste behavior, and microphone voice quality require platform validation.
A native macOS experiment confirms that showing the recording indicator preserves the foreground application and ignores pointer events.
The CI configuration defines builds for all three operating systems; this repository has no remote and CI has not run.
See [validation](docs/validation/README.md) for evidence and remaining acceptance work.

## Cleanup and privacy

Off keeps recognition output after exact mappings.
Basic removes configured standalone hesitation tokens and preserves quoted text, excluded terms, mapping output, and exact repository vocabulary.
Repository corrections remain suggestions until accepted.
Enhanced uses managed local S1-mini or an explicitly configured chat-completions endpoint.
It masks protected spans and checks placeholder order, selected constraint terms, and numeric literals.
These checks cannot prove semantic equivalence.
Errors, timeouts, oversized inputs, and failed preservation checks deliver the Basic result.
Conch does not send a later cleanup response after fallback or cancellation.

Remote cleanup starts disabled.
Enabling remote cleanup requires confirming the HTTPS destination in Settings.
Only the masked transcript and a fixed cleanup instruction leave the device.
The transcript can still contain confidential information, and the provider may charge for requests.
API keys use the platform credential store and can be removed in Settings.
Plain HTTP is accepted only for loopback local endpoints.
Conch never falls back from a local provider to a remote provider.

Settings and vocabulary indexes use a local SQLite database.
Repository files supply paths and declared symbols without executing repository code.
Conch keeps audio and the latest result in memory.
Conch discards audio after success or cancellation.
Failed recognition retains bounded audio in memory for Retry until another recording, cancellation, or exit.
Conch writes no transcript history or audio files.
Inference workers use authenticated loopback HTTP and are terminated after each stage or on cancellation.

## Check and package

```sh
npm test
npm run lint
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build
```

The runtime build script pins whisper.cpp and llama.cpp source revisions and disables external OpenSSL linkage for the local llama worker.
It adds bearer authentication and an 8 MiB request limit to the whisper worker.
Inference binaries and licenses ship as application resources.
Model weights download separately after installation.
The Windows and Linux runtime builds currently target CPU inference; macOS also builds Metal support.
No Windows or Linux acceleration path is validated.

macOS produces an application bundle and DMG under `src-tauri/target/release/bundle`.
Windows targets NSIS/MSI, and Linux targets AppImage/deb/rpm through Tauri.
macOS packages use local ad-hoc signing and are not notarized.
Public distribution requires a Developer ID identity, notarization credentials, and platform validation.
Model attribution and the S1-mini LICENSE and NOTICE ship in the app resources.
The S1-mini license adds a naming condition to Apache 2.0.

Synthetic inference evaluation uses macOS `say` and FFmpeg to create disposable test audio.
It requires the models under `.runtime` and writes the synthetic results to `docs/validation/synthetic-inference.json`.

```sh
python3 scripts/evaluate.py
cargo test --manifest-path src-tauri/Cargo.toml --test acceptance -- --ignored
```
