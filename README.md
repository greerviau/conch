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
Conch starts the recognizer when recording starts and recognizes each finished chunk while you keep speaking.
A chunk ends at the first pause of 400 ms after 8 seconds of audio, or at the quietest point once 25 seconds are pending.
Only the audio after the last chunk remains to be recognized when you stop.
If a chunk fails, Conch recognizes the whole recording once.
Workers stay running between recordings and stop after 10 minutes (recognizer) or 5 minutes (managed cleanup) without use.
Conch stops workers left by a crashed earlier run when it starts.
A floating indicator shows microphone startup, recording level, duration, processing, and the delivery outcome above other apps.
The indicator ignores pointer input and does not activate Conch.
It uses the monitor work area to stay above the Dock or taskbar, with 24 logical pixels of bottom clearance when space permits.
It refreshes its bounds while visible to follow work-area changes.
Settings can disable the indicator.
Minimize Conch to leave it available while dictating in other apps.
Closing its main window hides it and removes the Dock icon on macOS.
Conch keeps running with its menu bar icon, global shortcut, and recording indicator.
The icon shows a dot while recording and an ellipsis while preparing text, and its menu has Show Conch and Quit Conch.
Clicking the Dock icon or choosing Show Conch brings the window back.
If the menu bar icon cannot be created, closing the window quits Conch instead.
Quit Conch cancels active work and stops the inference workers.

Conch copies each initial nonempty result once.
Conch requests one paste when the same eligible text input remains focused across the delivery check.
It never sends Enter.
Uncertain or secure fields receive clipboard fallback.
Review edits and accepted suggestions require explicit copying.
Keep original rejects a proposed edit, and Undo reverses an accepted edit when its replacement remains uniquely identifiable after manual editing.
The raw transcript remains available until another result replaces it or Conch closes.

## Permissions and platform status

Click Enable microphone in setup to request microphone access on macOS.
Click Enable automatic paste in setup to open System Settings / Privacy & Security / Accessibility.
Enable Conch there for focused-input detection and paste.
macOS ties the Accessibility grant to the app's code signature.
An ad-hoc signature changes with every build, so each rebuilt copy loses the grant.
Build local copies with `npm run bundle:local` to keep one grant across rebuilds.
The script creates a self-signed "Conch Local Signing" identity in the login keychain once, signs with it, and fails if the bundle is still ad-hoc signed.
Pass `--install` to replace `/Applications/Conch.app` after the build.
The first copy signed this way needs one new grant; later builds reuse it.
An enabled grant left from an ad-hoc build stays attached to that older executable.
If Conch still reports missing access, expand Enabled in Settings but still not detected?
The repair action removes only Conch's Accessibility entry and opens Settings so you can enable the running copy.
The app displays that copy's installation path.
Restart Conch after enabling the current copy if the process still reports missing access.
Repair and restart are unavailable while recording, processing, downloading a model, or choosing a shortcut.
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
The CI workflow builds, lints, and tests on macOS, Windows, and Ubuntu for every push.
A passing run on all three is not recorded here; see the Actions tab for current results.
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
Inference workers use authenticated loopback HTTP.
They stay running between recordings, stop after an idle period or on exit, and cancellation stops the ones the cancelled recording used.

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

On macOS, install local builds with `npm run bundle:local -- --install` so the Accessibility grant survives rebuilds.
See [AGENTS.md](AGENTS.md) for the module layout, conventions, and known pitfalls.

The runtime build script pins whisper.cpp and llama.cpp source revisions and disables external OpenSSL linkage for the local llama worker.
It adds bearer authentication and an 8 MiB request limit to the whisper worker.
Inference binaries and licenses ship as application resources.
Model weights download separately after installation.
The Windows and Linux runtime builds currently target CPU inference; macOS also builds Metal support.
No Windows or Linux acceleration path is validated.

macOS produces an application bundle and DMG under `src-tauri/target/release/bundle`.
Windows targets NSIS/MSI, and Linux targets AppImage/deb/rpm through Tauri.
`tauri build` produces ad-hoc signed macOS packages, and `npm run bundle:local` produces locally signed ones; neither is notarized.
Public distribution requires a Developer ID identity, notarization credentials, and platform validation.
Model attribution and the S1-mini LICENSE and NOTICE ship in the app resources.
The S1-mini license adds a naming condition to Apache 2.0.

Synthetic inference evaluation uses macOS `say` and FFmpeg to create disposable test audio.
It requires the models under `.runtime` and writes the synthetic results to `docs/validation/synthetic-inference.json`.

```sh
python3 scripts/evaluate.py
cargo test --manifest-path src-tauri/Cargo.toml --test acceptance -- --ignored
```

## Logo and app icons

Conch uses a spiral shell mark with mint and coral colors.
The transparent vector logo supplies the app header and the generated desktop icons.
`npm run icons` regenerates the public SVG assets, desktop icon formats, and PNG exports from `assets/logo.svg`.
`assets/logo.png` contains the transparent 512-pixel logo, and `assets/app-icon.png` contains the 1024-pixel app icon.
The macOS bundle contains the ICNS icon, and Windows uses the ICO icon.
Quit the installed app before replacing it so the Dock can load the updated icon.
