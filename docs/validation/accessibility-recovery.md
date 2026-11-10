# Accessibility grant recovery

## Reproduction

The installed and running Conch 0.2.0 executable has code hash `b9d8f78e57371d5dd4ab04f002d5dcf7716414ca` at the time of diagnosis.
The running process hash, obtained through `csops`, matches the installed executable's signature.
The macOS Accessibility record for `app.conch.desktop` has `auth_value=2`, but its stored requirement contains the older hash `1c3caf9a20579ac1fd8e652b9c77fa55ef564adf`.
A `codesign --verify -R` check fails against the stored requirement and passes against the running build's requirement.
The enabled switch therefore does not authorize this running copy.
The native trust check remains the authority for whether Conch can use Accessibility.
The app does not treat the enabled Settings switch as permission for a different executable.

Apple documents that an ad-hoc designated requirement identifies a specific version of the code in [TN3127](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements/).
Builds made with plain `tauri build` use ad-hoc signing, so each changed executable can need a new grant.
`npm run bundle:local` signs with a persistent self-signed identity instead.
Its designated requirement is `identifier "app.conch.desktop" and certificate leaf = H"<identity hash>"`, which does not change between builds.
The script compares the bundle's requirement with that value and fails on a mismatch.
Signing two different binaries with the identity produced identical requirements.
Whether the system Accessibility database accepts the grant across rebuilds was not tested here, because the grant needs the user's action in System Settings.
A stable signing identity remains necessary to preserve code identity across release updates.
Conch does not install a certificate or change system trust settings as part of recovery.

## Recovery controls

Setup and Settings offer Enabled in Settings but still not detected? when the current process is untrusted.
The instructions identify the running app bundle and explain how an old enabled entry can remain after an update.
Repair Accessibility entry first displays the scope and requires a separate Remove old entry and open Settings action.
The backend runs `/usr/bin/tccutil reset Accessibility app.conch.desktop` and opens the current process's permission request only after a successful reset.
The action affects Conch's Accessibility entry and does not reset other apps or other permission services.
The user enables Conch again in System Settings.
The UI continues to poll the native trust check and updates when that check reports access.

Restart Conch is a separate action for rechecking access in a fresh process.
It does not reset the Accessibility entry.
The UI explains that restart discards the in-memory result and any recording kept for Retry.
The backend rejects repair and restart while recording, processing, downloading a model, or capturing a shortcut.

## Regression evidence

- The initial component test cannot find the stale-grant recovery instructions or the restart action.
- The initial scoped reset implementation returns Repair is unavailable instead of issuing the requested operations.
- The fixed tests require explicit confirmation before reset and verify that opening Settings alone does not reset access.
- The fixed workflow test requires the exact executable, Accessibility service, and Conch bundle identifier, followed by the permission request.
- A failed reset does not issue a new permission request.
- The rendered UI changes to enabled when the native permission value changes to true.
- Restart calls its own action without resetting the permission entry, and recovery controls are disabled during active work.

Tests use an injected command runner and do not change the user's permission database.
The diagnosis reads only Conch's existing Accessibility record.
The validation run does not grant permission or reset the installed app's grant automatically.
Live insertion still requires the user to authorize the installed executable and permit native UI automation for the acceptance run.
