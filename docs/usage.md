# Using Conch

## Setup

The Dictation page shows the setup checklist. Its first step has a Language menu, a menu of the recognition models that support that language, and a button that names the selected model, for example Download Small (English), 487.6 MB.
English and Small (Whisper small.en, 487.6 MB) are the defaults for a new install. Picking another language or model there saves it at once.
Once a model is installed the step names it and offers Change model.
Enable microphone access and automatic paste from the same checklist.
The paste step disappears while Automatic paste is off in Settings.
Conch reports permission status when you return from System Settings.
The optional S1-mini by Superwhisper download enables managed Enhanced cleanup.
Conch verifies model size and SHA-256 before installation.
Model files remain in Conch's local application data directory.

## Choosing a recognition language

Settings has a Language menu with English first and then every language in the Whisper language table (100 entries including English), sorted by name.
English is the default, and settings saved before the menu existed load as English.
Auto-detect is the first entry of the menu. It lets the recognizer detect the language, and Settings offers it while a multilingual model exists. Choosing it keeps a multilingual model, and otherwise selects Small.
With Auto-detect, the first chunk of a recording that has speech is recognized with detection, and the rest of that recording reuses the language it found, because short chunks are often misdetected. The next recording detects again.
Auto-detect is less reliable for recordings under a few seconds. On synthetic speech in eight languages, Small identified the language of 48 of 48 sentences of 1.6 s or longer but only 38 of 48 phrases under 1.6 s, and a misdetected chunk comes back in the wrong language. Choose a language in Settings when you can. `docs/validation/recognition-languages.md` has the measurements.
Auto-detect also turns off hesitation-token removal, spelled-letter matching, and managed Enhanced cleanup, each of which needs a known language.
The recognition hint still goes to the recognizer with Auto-detect. It does not affect which language is detected, because the recognizer detects from the audio before it reads the hint.
The Recognition model menu lists only the models that support the chosen language.
Changing the language keeps the current model when it supports the new language, and otherwise selects Small, or Small (English) for English.
Settings saved with a model that does not support the saved language load with the smallest model that does, and Dictation asks for its download.
Conch sends the language with each recognition request, so a change takes effect on the next recording without restarting the recognizer.
A recording keeps the language and model it started with, and Retry recognizes the kept recording the same way.
The interface stays in English.
Conch makes no accuracy claim per language: Whisper's error rates differ widely between languages.
Mixed speech, such as prose in one language with English identifiers, was measured on five synthetic sentences only, and human code-switching was not measured.

## Choosing a recognition model

Settings has a Recognition model menu with the English models Tiny (77.7 MB), Base (148.0 MB), and Small (487.6 MB), and the multilingual models Tiny (77.7 MB), Base (148.0 MB), Small (487.6 MB), and Large Turbo (574.0 MB, quantized q5_0).
English-only models recognize English alone. Multilingual Tiny, Base, and Small recognize 99 languages, every one in the table but Cantonese. Large Turbo recognizes all 100, so choosing Cantonese selects it.
Larger models make fewer errors and take longer to finish after you stop speaking.
`docs/validation/recognition-models.md` has the measurements.
The menu shows Installed in place of the size for a model on disk.
Under the menu, the selected model shows Installed, a Download button, or a progress bar. Settings has no separate list of models.
Enhanced cleanup shows the same status for S1-mini once Managed S1-mini is chosen.
Dictation asks for the download until the saved model is installed.
Saving a different model, or a language setting that moves to or from Auto-detect, stops the idle recognizer, and the next recording starts a fresh one.
Retry recognizes the kept recording with the model the recording started with.

Every recognition model download also installs the 0.9 MB Silero voice detection model.
The recognizer skips audio that voice detection classifies as silence or noise, so a pause or room noise after you stop speaking does not turn into words such as "Thank you."
Copies that installed Tiny before this model existed keep Tiny and show Download Voice detection (Silero), 0.9 MB once to fetch it.

## Cleanup

Basic cleanup always runs. It removes the hesitation tokens in Settings and repairs the spacing and punctuation around them.
Each recognition language has its own list. English starts with `um` and `uh`, and every other language starts empty, because a shared list would delete real words: `um` is a Portuguese article and a German preposition. Edit the list for the language shown in the Language menu.
A hesitation list saved before languages existed loads as the English list.
Auto-detect removes no hesitation tokens, and Settings disables the row and says why.
Leave Hesitation tokens blank and Basic cleanup leaves the transcript exactly as recognized.
Excluded literal terms stay as dictated.
Enhanced cleanup is Disabled until you choose Managed S1-mini, Local endpoint, or Remote endpoint.
Managed S1-mini asks for the 484 MB download; until it is installed, Conch uses Basic cleanup.
Managed S1-mini supports English only. For any other language and for Auto-detect, Conch uses Basic cleanup, the result status says so, and Settings shows the reason. A local or remote endpoint works in every language.
Settings saved by an earlier version start with Enhanced cleanup Disabled.
With Enhanced cleanup on, Conch cleans each finished chunk while you keep speaking, one at a time.
After you stop, it recognizes and cleans the audio after the last chunk, behind any chunk whose cleaning is still running; a slow endpoint can leave a backlog.
A dictation with no chunk is recognized and cleaned whole after the stop. That is one shorter than about 9 seconds, or one with no pause of a second after 8 seconds and under 25 seconds of audio pending.
If a mapping or a quotation spans two chunks, Conch cleans the whole text after the stop, because each chunk was protected alone.
A part that Basic cleanup leaves empty adds nothing to the result.
If cleaning any part fails, the whole result gets Basic cleanup and the status line gives the reason.

## Microphone and paste

Microphone in Settings starts at System default, which follows the input your system uses, including when you switch it.
Choose a device to always record from it. If it is unplugged, Settings says so and Conch records from the system input.
Settings lists the devices when you open it, so reopen Settings after plugging one in.
Automatic paste is on by default. Turn it off and Conch only copies each result; paste it yourself.
With Automatic paste off, Conch does not ask for the Accessibility permission.

## Repositories, mappings, and the shortcut

The top bar switches between Dictation, History, Repositories, Mappings, Settings, and Logs.
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
Spelled-letter matching, which turns "pod gee see" into `podGC`, uses English letter names and works for English only. In other languages and in Auto-detect, a recognizer that writes the capitals ("pod GC") still matches, and mapping and identifier matching ignore case for every script by Unicode simple case folding, the same rule the mapping patterns use. Sound-alike matching also reads the German sharp s (U+00DF) as `ss` and treats the Greek capital sigma (U+03A3), small sigma (U+03C3), and final sigma (U+03C2) as one letter. A combining mark stays with its letter. Conch does not normalize text, so a precomposed letter and a letter followed by a separate combining mark count as different spellings.
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
If a chunk fails, Conch recognizes the whole recording once.
The Logs page records the time from the stop to the text and how long each stage took: capture, recognition, preparation, cleanup, and delivery. Cleanup is the wait for the tail's cleaning when chunks were cleaned during the recording; it appears only with Enhanced cleanup. With Managed S1-mini it also shows the time the model reported for its latest request, for example `cleanup 542 ms (model 290 ms)`, so a slow model shows apart from slow work around it. When recording starts, Conch sends the model one token so it is loaded before the first cleanup.
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

## Logs

The Logs page shows what Conch did and why, newest first, 50 entries at a time.
Each entry has a time, a level (Info, Warning, or Error), a source, and a message, and some have details behind a Details link.
The sources are `app`, `cleanup`, `delivery`, `dictation`, `download`, `index`, `permissions`, `recognizer`, `ui`, and `update`.
Conch logs each dictation's stop-to-text time with its stages, how each result was delivered and why a clipboard fallback happened, cleanup and recognition failures, inference worker starts, stops with the reason, and failures with the recognizer's last diagnostic lines, errors that the Dictation page showed for a failed command (source `ui`), model downloads, update checks, permission changes, and repository indexing.
The buttons above the list narrow it to warnings (warnings and errors) or to errors, and to one source. Search matches the message, the details, and the source.
Copy shown entries puts the entries on the page on the clipboard as text with UTC times, which suits a bug report.
Show log file opens the file manager at `conch.log` in Conch's data directory.
Clear logs asks for confirmation and deletes every entry.
Conch writes the log whether or not Keep history is on, keeps about 4 MB (`conch.log` and the previous `conch.log.1`), and deletes the oldest entries first.
Logs never contain audio, transcripts, result text, or source text. They show your home directory as `~` and any other absolute path as `.../file name`; see [privacy](privacy.md).
The Dictation page links its status line and its error banner to this page.

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
