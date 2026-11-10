# Development, packaging, and branding

See [AGENTS.md](../AGENTS.md) for the module layout, conventions, and known pitfalls.

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
