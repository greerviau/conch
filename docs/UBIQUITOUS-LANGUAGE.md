# Conch terminology

- Conch: the local cross-platform desktop dictation app that uses configured repositories and explicit mappings to prepare agent prompts.
- Repository context: the selected local repository vocabulary available to one recording. It excludes foreground-app state and unselected repositories.
- Context snapshot: the fixed vocabulary and mapping versions captured when a recording starts.
- Explicit mapping: a user-defined spoken phrase or transcription-error phrase paired with exact replacement text, scoped globally or to a repository.
- Vocabulary index: locally extracted filenames, relative paths, and declared symbols with their repository and file origins.
- Raw transcript: the speech recognizer's output before explicit mappings or repository corrections.
- Repository suggestion: a proposed transcript edit derived from indexed vocabulary that requires user acceptance.
- Result: the editable transcript after mappings and configured cleanup, with optional accepted suggestions and manual edits. Initial delivery copies it automatically and pastes it into an eligible focused input; subsequent review edits require explicit copying.
- Push-to-talk: recording activation mode that starts on hotkey press and finishes on release.
- Toggle recording: recording activation mode that starts on one hotkey press and finishes on the next.
- Eligible focused input: the currently focused text-editable element that the app can identify with sufficient certainty and that is not a secure/password field.
- Clipboard fallback: delivery that retains the result on the clipboard when automatic paste cannot be performed.
- Basic cleanup: local deterministic removal of configured hesitation tokens with limited whitespace/punctuation repair.
- Enhanced cleanup: optional model-assisted transcript cleanup using managed local S1-mini or a configured local/remote endpoint, with Basic fallback on failure.
- Protected span: transcript text that mappings or exact repository matches identify for preservation through cleanup.
