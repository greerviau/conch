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

The top bar switches between Dictation, Repositories, Mappings, and Settings.
On Repositories, click Add repository to choose a folder, or Enter a path instead to type one.
Changes to repositories, mappings, and settings stay in a draft until you click Save changes in the bar that appears at the bottom.
After saving, click Refresh indexes.
Each repository shows Ready, Partial index, or Folder not found, its term count, when it was last indexed, and how many symbol extractions were skipped.
Click Terms to browse what Conch indexed for a repository: file paths, file names, and function, class, and type names.
Search the terms, filter them to Files or Symbols, and move through them 100 at a time with Previous and Next.
Click Exclusions to add ignore patterns, one per line.
Select the repositories that supply context on the Dictation page before recording.
Define global or repository mappings for recurring transcription errors on Mappings.
Each mapping has an enable switch, the spoken phrase, the replacement text, and a scope.
Conch also sends the recognizer a short list of identifiers from the selected repositories and your mappings, so it spells them correctly.
If a repository suggestion appears for a phrase you say often, choose Accept and always replace. Conch saves a mapping for that phrase and applies it from the next recording.
The default shortcut is Command-or-Control + Shift + Space in push-to-talk mode.
In Settings, click Change shortcut and press the desired combination.
Conch pauses its registered shortcut during capture and ignores repeat events.
Escape or leaving the control cancels capture.
Click Save changes to register the new shortcut.
The in-app recording buttons work independently of shortcut availability.

## Recording

Recordings stop after 180 seconds.
Conch starts the recognizer when recording starts and recognizes each finished chunk while you keep speaking.
A chunk ends at the first pause of one second after 8 seconds of audio, or at the quietest point once 25 seconds are pending.
Only the audio after the last chunk remains to be recognized when you stop.
If a chunk fails, Conch recognizes the whole recording once.
Workers stay running between recordings and stop after 10 minutes (recognizer) or 5 minutes (managed cleanup) without use.
Conch stops workers left by a crashed earlier run when it starts.

## Recording indicator

A small dark pill floats above other apps while you dictate.
It shows animated level bars and the elapsed time while recording.
While Conch prepares the result, the bars become a single line that draws the conch spiral from its center, then erases from the center.
When Conch pastes the result, the pill disappears without a message.
It shows text only when the result was copied but not pasted, when there was no text to copy, or when something failed.
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
Installing downloads the release, waits for any recording to finish, stops the workers, installs, and restarts Conch.
Settings has a switch for the automatic check, and Check for updates in Settings or the menu bar item looks on demand.
macOS keys the Accessibility grant to the signing identity, so an update keeps it only when the release is signed with the same identity as the installed copy.
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
