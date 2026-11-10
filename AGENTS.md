# Conch agent guide

Conch is a cross-platform desktop dictation app built with Tauri 2, React/TypeScript, and Rust.
Read `README.md` and `docs/usage.md` for behavior, `docs/plans/2026-11-09-conch.md` for the spec, and `docs/UBIQUITOUS-LANGUAGE.md` for terms.
Use the glossary's terms verbatim and update the spec and glossary when a decision changes.

## Layout

- `src/`: React interface with Vitest component tests beside each file. `App.tsx` holds the top bar, Dictation, and Settings; `Repositories.tsx`, `Terms.tsx`, and `Mappings.tsx` are the other pages. `Setup.tsx` and `PasteAccess.tsx` are the setup checklist, `RecordingIndicator.tsx` and `Spiral.tsx` the floating indicator, `style.css` the light and dark themes.
- `src-tauri/src/app.rs`: Tauri commands, shared state, the recording flow, and `process` (recognition result to delivery).
- `src-tauri/src/inference.rs`: worker pool (`acquire`, `preload`, `reap_idle_within`, `shutdown`, `reap_orphans`), recognizer and cleanup requests, model downloads.
- `src-tauri/src/chunk.rs`: splits a growing recording at pauses. `pipeline.rs`: recognizes chunks on a background thread. `audio.rs`: microphone capture and WAV encoding.
- `src-tauri/src/core.rs`: mappings, masking, Basic cleanup, suggestions, result edits. `index.rs`, `storage.rs`: repository vocabulary and SQLite.
- `src-tauri/src/platform.rs`, `macos.m`, `windows.rs`: focused-field detection, paste, permissions. `delivery.rs`: the copy-then-paste decision.
- `src-tauri/src/update.rs`: update check, install, and restart with `tauri-plugin-updater`. `scripts/check-version.mjs`, `scripts/verify-signing.sh`, and `.github/workflows/release.yml` publish releases.
- `src-tauri/src/tray.rs`: menu bar icon, state display, show and hide of the main window. `indicator.rs`: floating recording indicator.
- `scripts/`: runtime build (`build-runtimes.py`), brand assets, `setup-local-signing.sh`, `bundle-local.sh`.
- `docs/validation/`: evidence and measurements. Do not state a result there that was not observed.

## Commands

```sh
npm ci
python3 scripts/build-runtimes.py   # builds src-tauri/workers; needs CMake and a C++ toolchain
npm test && npm run lint && npm run check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run bundle:local -- --install   # macOS: build, verify signing, replace /Applications/Conch.app
```

Acceptance tests marked `#[ignore]` need `.runtime/` (models and a synthetic WAV) and built workers: `cargo test --test acceptance -- --ignored`.
`src-tauri/examples/latency.rs` measures stop-to-text latency on real workers.

## Conventions

- Use `uv` for Python tooling, never plain `pip`.
- Ignore backward compatibility unless asked.
- Fix every lint error, test failure, and flaky test you encounter, including unrelated ones.
- Write tests first for behavior changes and record the failing run.
- Do not add an agent as a commit co-author or mark PR descriptions as generated. Do not comment on PRs unless asked.
- Write prose in ASCII. Report only what was run. Separate tested macOS behavior from untested Windows and Linux support.

## Pitfalls

- **Signing and Accessibility.** macOS keys the Accessibility grant to the code signature. Plain `tauri build` signs ad-hoc, so every rebuild loses the grant. Install local builds with `bundle:local`, which signs with the "Conch Local Signing" identity and fails if the requirement is not `identifier "app.conch.desktop" and certificate leaf = H"<hash>"`.
- **Updater artifacts.** `createUpdaterArtifacts` is on, so a bundle build without `TAURI_SIGNING_PRIVATE_KEY` fails after it finishes. Local and check builds pass `--config '{"bundle":{"createUpdaterArtifacts":false}}'`. The updater private key and password never go in the repository.
- **Restart.** `app.restart()` exits without the exit event, so `update::install_now` stops the workers itself before calling it.
- **Electron apps.** Electron and Chromium expose no accessibility tree until `AXManualAccessibility` is set on the app element. `platform.rs` gets the frontmost pid from `NSWorkspace` and wakes that app, because the system-wide focused-application query fails before the wake.
- **Agent shells have no Accessibility trust.** Focus detection and paste cannot be probed from an agent session, and a binary signed with Conch's identity does not inherit the grant when run from there. Ask the user to test the installed build. The grey status line under the result box names the reason for a clipboard fallback.
- **Workers.** `whisper-server` needs `CONCH_WORKER_TOKEN` and bearer authentication when run by hand. Metal and loopback servers fail inside a restrictive sandbox. Workers are pooled per process: tests must scope `shutdown` and `reap_idle_within` to their own temp directory, or parallel tests kill each other's workers. A test process that exits without `shutdown` leaks workers (the llama worker holds about 1 GB).
- **Recognizer output.** whisper puts each segment on its own line. `flatten_segments` joins them with spaces at the recognizer boundary.
- **Cancellation.** `Worker` is per recording and records the servers it uses. `stop` kills those servers; the pool respawns on the next request.
- **Disk.** A release Tauri build needs several GB. Do not clone a `target/` directory; set `CARGO_TARGET_DIR` to an existing cache.
- **Worker tests** that use a fake server need `python3` on Unix and are skipped on Windows.
