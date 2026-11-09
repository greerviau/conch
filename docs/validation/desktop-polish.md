# Desktop setup and control validation

## Reproductions

The initial setup component has no recognition-model download action on the Dictation page.
The initial shortcut control is a text input and cannot capture a key combination.
The first component tests fail while looking for those controls.
The macOS focus diagnostic reports `accessibility_granted=false` and `eligible_focus=false` for the diagnostic process in this environment.
The user reports that Accessibility was not enabled or its state was uncertain in the installed app.
That reply does not establish the installed app's exact permission state.

The original input policy requires a writable whole-value Accessibility setter.
The regression fixture supplies a text area with a writable selected-text setter and fails eligibility under that policy.
The corrected policy accepts either setter and excludes secure, disabled, read-only, and Conch-owned inputs.
Delivery checks the current focused element again after copying and reports the clipboard fallback reason.

## Setup and settings checks

Component tests exercise these flows through rendered React controls and mocked Tauri commands.

- The Dictation page offers the required recognition-model download directly and keeps recording disabled until prerequisites are ready.
- The required download does not start an optional cleanup-model download.
- Setup offers the automatic-paste permission action.
- Clicking Change shortcut suspends Conch's registered shortcut and captures a modifier/key chord.
- Repeat and modifier-only events do not save shortcuts, and Escape cancels capture.
- Shortcut registration resumes after all captured keys are released.
- Leaving Settings while shortcut suspension is pending restores the registered shortcut.
- Capturing a chord restores the old shortcut before Save changes registers the replacement.
- The recording indicator displays status and elapsed time without interactive controls.

The UI exposes a folder picker, pending settings changes, discard controls, explicit model/permission readiness, and a disabled Retry control when no failed audio exists.
The setup page explains that repositories and Enhanced cleanup are optional.
Enhanced settings offer a download action when managed cleanup needs its optional model.

## Native recording indicator experiment

`cargo run --manifest-path src-tauri/Cargo.toml --example overlay_focus` exercises the same macOS overlay-visibility function that the app uses.
It creates a borderless window, records the foreground process, shows the overlay, runs the native event loop, and checks the foreground process again.
The experiment passes on this Mac.
The overlay is visible, ignores mouse events, does not become a key window, and preserves the foreground application.
The app initially chooses the pointer's monitor and centers the indicator near the bottom of its work area.
It leaves 24 logical pixels above that area's bottom when space permits and clamps the window size to small work areas.
While visible, it refreshes placement on the indicator's current monitor as the Dock or taskbar changes the usable bounds.
The native window remains available to other Spaces and fullscreen auxiliary windows.
Fullscreen and multiple-monitor behavior still need receiving-app validation.

Presentation tests cover microphone startup, push-to-talk, toggle mode, processing, and idle visibility.
The audio callback supplies a bounded input-level value without storing additional audio.
The indicator receives status and level events and receives no transcript or source text.

## Native acceptance limits

The desktop automation host lists the installed application, but opening its Accessibility surface times out.
A later attempt to inspect TextEdit returns the explicit error "Computer Use permissions are not granted".
The native diagnostic process lacks Accessibility permission, so this run cannot validate inserting text into another application's input.
The user must enable Conch through the setup action before automatic paste can work.
Microphone permission and representative human recordings remain necessary for voice-quality acceptance.
The component tests do not establish native insertion, microphone capture quality, fullscreen behavior, or Windows/Linux desktop behavior.

The local Docker CLI cannot connect because its daemon socket is missing.
Both linked machines identify as Linux cloud hosts.
They do not establish access to an interactive X11, Wayland, or Windows receiving application.
The Windows native adapter has an isolated cross-target typecheck from the previous validation run.
Cross-platform CI remains configured and unexecuted because this repository has no remote.

## Regression record

- The initial setup test cannot find Download recognition model.
- The initial shortcut tests cannot find Change shortcut or a capture control.
- The initial indicator tests return invisible/empty state for recording and microphone startup.
- The selected-text setter fixture fails the original input-eligibility check.
- The pending-unmount fixture fails to restore the registered shortcut after its control disappears.
- The captured-key-release fixture restores the shortcut while the captured keys are still held.
- Review fixtures expose no-op microphone actions on Windows/Linux and unsupported paste-permission controls on Linux.
- The macOS setup shortcut label incorrectly shows Ctrl for Command-or-Control before its platform flag is supplied.

Each corresponding regression passes after its implementation change.
The native macOS overlay experiment and bundled inference/model-download integration checks also pass.

Independent review also corrects the microphone setup instructions to match the explicit permission request in Setup.
Windows/Linux now describe system-managed microphone access and omit the no-op permission action.
Linux Settings describes clipboard delivery and omits the unsupported Accessibility action.

## Checks for 0.2.0

- The frontend suite passes 13 component tests and the desktop configuration harness test.
- The Rust suite passes 20 unit tests and six integration tests.
- The opt-in packaged-worker test and verified model-download test pass separately.
- TypeScript, ESLint, Prettier, Rust formatting, Clippy, and diff whitespace checks pass.
- The macOS application bundle and DMG build with local ad-hoc signing.
- The native overlay focus-preservation experiment passes after the final Rust changes.

Full v1 acceptance remains blocked on permission-enabled native insertion, representative recordings, and the untested release desktops.
The app's setup controls request the microphone permission and open macOS Accessibility settings; they cannot grant the user's system permissions.

## Dock overlap reproduction and regression

The supplied screenshot shows the Dock covering the lower part of the indicator.
The native screen query reports an 85-point Dock reservation while the previous placement leaves only 50 points below the popup.
That places 35 points of the popup inside the Dock's reserved area.
The initial work-area tests reproduce overlap, incorrect placement with Retina/negative monitor origins, and failure to fit a narrow work area.
The corrected tests pass for bottom and side reservations, mixed origins, and fractional display scaling.
The native experiment positions a real borderless window using the Rust calculation, checks that its full frame fits inside `NSScreen.visibleFrame`, and verifies 24-point Dock clearance and unchanged foreground application.
That experiment passes on the current display without changing Dock settings or requiring Accessibility access.

The vector shell logo and rounded app icon replace the placeholder letter.
The icon generator produces PNG, ICNS, and ICO outputs, and the exported logo PNG retains transparency.
The UI uses the transparent mark beside the Conch name.
