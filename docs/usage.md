# Using Conch

## Setup

The Dictation page shows the setup checklist and a Download recognition model button.
Download Whisper tiny.en there for English recognition.
Enable microphone access and automatic paste from the same checklist.
Conch reports permission status when you return from System Settings.
The optional S1-mini by Superwhisper download enables managed Enhanced cleanup.
Conch verifies model size and SHA-256 before installation.
Model files remain in Conch's local application data directory.

## Repositories, mappings, and the shortcut

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

## Recording

Recordings stop after 180 seconds.
Conch starts the recognizer when recording starts and recognizes each finished chunk while you keep speaking.
A chunk ends at the first pause of 400 ms after 8 seconds of audio, or at the quietest point once 25 seconds are pending.
Only the audio after the last chunk remains to be recognized when you stop.
If a chunk fails, Conch recognizes the whole recording once.
Workers stay running between recordings and stop after 10 minutes (recognizer) or 5 minutes (managed cleanup) without use.
Conch stops workers left by a crashed earlier run when it starts.

## Recording indicator

A floating indicator shows microphone startup, recording level, duration, processing, and the delivery outcome above other apps.
The indicator ignores pointer input and does not activate Conch.
It uses the monitor work area to stay above the Dock or taskbar, with 24 logical pixels of bottom clearance when space permits.
It refreshes its bounds while visible to follow work-area changes.
Settings can disable the indicator.
Minimize Conch to leave it available while dictating in other apps.

## Background operation

Closing its main window hides it and removes the Dock icon on macOS.
Conch keeps running with its menu bar icon, global shortcut, and recording indicator.
The icon shows a dot while recording and an ellipsis while preparing text, and its menu has Show Conch and Quit Conch.
Clicking the Dock icon or choosing Show Conch brings the window back.
If the menu bar icon cannot be created, closing the window quits Conch instead.
Quit Conch cancels active work and stops the inference workers.

## Updates

Conch checks GitHub for a newer release shortly after launch and then every 24 hours.
When one exists, the menu bar item reads Update to X.Y.Z and Settings offers an Install button.
Installing downloads the release, waits for any recording to finish, stops the workers, and restarts Conch.
Settings has a switch for the automatic check, and Check for updates in Settings or the menu bar item looks on demand.
Updates keep the macOS Accessibility grant when the release and the installed copy use the same signing identity.
A copy you built yourself switches to the official build on its first update.
Only an AppImage updates itself on Linux.

## Delivery

Conch copies each initial nonempty result once.
Conch requests one paste when the same eligible text input remains focused across the delivery check.
It never sends Enter.
Uncertain or secure fields receive clipboard fallback.
Review edits and accepted suggestions require explicit copying.
Keep original rejects a proposed edit, and Undo reverses an accepted edit when its replacement remains uniquely identifiable after manual editing.
The raw transcript remains available until another result replaces it or Conch closes.

See [permissions](permissions.md) for what paste needs and [privacy](privacy.md) for what leaves the device.
