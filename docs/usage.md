# Using Conch

## Setup

The Dictation page shows the setup checklist. Its first step has a menu of the three recognition models and a button that names the selected one, for example Download Small (English), 487.6 MB.
Small (Whisper small.en, 487.6 MB) is the default for a new install. Picking another model there saves it at once.
Once a model is installed the step names it and offers Change model.
Enable microphone access and automatic paste from the same checklist.
The paste step disappears while Automatic paste is off in Settings.
Conch reports permission status when you return from System Settings.
The optional S1-mini by Superwhisper download enables managed Enhanced cleanup.
Conch verifies model size and SHA-256 before installation.
Model files remain in Conch's local application data directory.

## Choosing a recognition model

Settings has a Recognition model menu with three English models: Tiny (77.7 MB), Base (148.0 MB), and Small (487.6 MB).
Larger models make fewer errors and take longer to finish after you stop speaking.
`docs/validation/recognition-models.md` has the measurements.
The menu shows Installed in place of the size for a model on disk.
Under the menu, the selected model shows Installed, a Download button, or a progress bar. Settings has no separate list of models.
Enhanced cleanup shows the same status for S1-mini once Managed S1-mini is chosen.
Dictation asks for the download until the saved model is installed.
Saving a different model stops the idle recognizer, and the next recording starts the new one.
Retry recognizes the kept recording with the model selected when you retry.

Every recognition model download also installs the 0.9 MB Silero voice detection model.
The recognizer skips audio that voice detection classifies as silence or noise, so a pause or room noise after you stop speaking does not turn into words such as "Thank you."
Copies that installed Tiny before this model existed keep Tiny and show Download Voice detection (Silero), 0.9 MB once to fetch it.

## Cleanup

Basic cleanup always runs. It removes the hesitation tokens in Settings, `um` and `uh` at first, and repairs the spacing and punctuation around them.
Leave Hesitation tokens blank and Basic cleanup leaves the transcript exactly as recognized.
Excluded literal terms stay as dictated.
Enhanced cleanup is Disabled until you choose Managed S1-mini, Local endpoint, or Remote endpoint.
Managed S1-mini asks for the 484 MB download; until it is installed, Conch uses Basic cleanup.
Settings saved by an earlier version start with Enhanced cleanup Disabled.

## Microphone and paste

Microphone in Settings starts at System default, which follows the input your system uses, including when you switch it.
Choose a device to always record from it. If it is unplugged, Settings says so and Conch records from the system input.
Settings lists the devices when you open it, so reopen Settings after plugging one in.
Automatic paste is on by default. Turn it off and Conch only copies each result; paste it yourself.
With Automatic paste off, Conch does not ask for the Accessibility permission.

## Repositories, mappings, and the shortcut

The top bar switches between Dictation, History, Repositories, Mappings, and Settings.
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
To skip the review step, turn on Auto-accept under Repository context in Settings. Conch then replaces a heard phrase with the identifier it sounds like whenever the words match one indexed spelling, so the pasted text contains the symbol name. A phrase that matches several different spellings, overlaps another suggestion, or conflicts with mappings still waits for review. Each replacement is listed under Accepted edits with an Undo button; Undo changes the result in Conch only, so copy it again to replace text that was already pasted. Auto-accept is off by default, and a wrong match is possible because the matching is by sound.
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
When speech is followed by 0.3 seconds of silence, Conch recognizes the audio after the last chunk without waiting for the stop.
If you stop without speaking again, Conch uses that text and the stop needs no recognition.
If you speak again first, Conch discards it and recognizes the audio after the stop as usual.
If a chunk fails, Conch recognizes the whole recording once.
The status line under the result ends with the time from the stop to the text and how long each stage took: capture, recognition, preparation, cleanup (Enhanced cleanup only), and delivery.
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
The icon shows a dot while recording and an ellipsis while preparing text, and its menu has Show Conch, Check for Updates, the installed version, and Quit Conch.
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

## History

Conch keeps only the latest result unless you turn on Keep history under History in Settings and click Save changes. It is off by default.
With it on, Conch saves each delivered result with its raw transcript and the time, up to 500 entries; the oldest goes first.
Edits and accepted suggestions on the Dictation page update the saved text.
The History page lists entries newest first, 50 at a time. Search matches the result or the raw transcript.
Copy puts an entry's result on the clipboard, Copy raw puts the raw transcript there when it differs, and neither pastes.
Delete removes one entry. Clear history asks for confirmation and removes all of them.
Turning Keep history off stops saving but keeps what is already there until you delete it.
Entries are stored unencrypted in Conch's local database; see [privacy](privacy.md).

## Delivery

Conch copies each initial nonempty result once.
While Automatic paste is on, Conch requests one paste when the same eligible text input remains focused across the delivery check.
With Automatic paste off, it only copies the result and the status line says so.
It never sends Enter.
Uncertain or secure fields receive clipboard fallback.
Review edits and accepted suggestions require explicit copying.
Keep original rejects a proposed edit, and Undo reverses an accepted edit when its replacement remains uniquely identifiable after manual editing.
The raw transcript remains available until another result replaces it or Conch closes.

See [permissions](permissions.md) for what paste needs and [privacy](privacy.md) for what leaves the device.
