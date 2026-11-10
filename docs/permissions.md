# Permissions and platform status

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
The CI workflow builds, lints, and tests on macOS, Windows, and Ubuntu. It runs only when started by hand from the Actions tab.
A passing run on all three is not recorded here; see the Actions tab for current results.
See [validation](validation/README.md) for evidence and remaining acceptance work.
