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
npm run tauri -- build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Updater artifacts are on in `tauri.conf.json`, so a bundle build fails without `TAURI_SIGNING_PRIVATE_KEY` unless it passes the override above. `bundle:local` and the check workflow already do.

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

## Releasing

Releases come from GitHub Actions, so no release needs a Mac or an Apple Developer account.

One-time setup, as repository secrets:

- `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the updater key from `npm run tauri -- signer generate`. Its public half is the `pubkey` in `tauri.conf.json`. Keep a copy outside GitHub: without it, installed copies can never update.
- `APPLE_CERTIFICATE` (base64 of a `.p12`), `APPLE_CERTIFICATE_PASSWORD`, and `APPLE_SIGNING_IDENTITY` (the certificate's SHA-1 hash): the "Conch Local Signing" identity. Export it from Keychain Access as a `.p12` and run `base64 -i conch.p12`. Every release must use this same certificate, because the Accessibility grant is keyed to it.

To release:

1. Set the same version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, and commit.
2. Push a tag `vX.Y.Z`. The version job fails if the tag and manifests disagree, and the macOS job fails if the app is not signed with the stable identity.
3. Open the draft release, check the installers and `latest.json`, and publish it. Installed copies see it only after this step.

The macOS release is built on Apple Silicon only (`macos-15`), so Intel Macs get no updater artifact.
`scripts/install.sh` downloads `Conch_aarch64.app.tar.gz` and the `*_amd64.AppImage` asset by name, so keep those names when changing the bundle targets.
The first updater build must be installed by hand; earlier copies cannot update themselves.
Forks replace `pubkey` and `endpoints` in `tauri.conf.json` and use their own keys.
To build a copy that never updates, clear `endpoints` with `--config '{"plugins":{"updater":{"endpoints":[]}}}'`.
Notarization and Windows signing are not set up, so first installs from a browser show Gatekeeper and SmartScreen warnings.

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
