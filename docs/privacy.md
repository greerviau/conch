# Cleanup and privacy

Basic with no hesitation tokens keeps recognition output after exact mappings.
Basic with tokens removes configured standalone hesitation tokens and preserves quoted text, excluded terms, mapping output, and exact repository vocabulary.
Repository corrections remain suggestions until accepted.
Enhanced uses managed local S1-mini or an explicitly configured chat-completions endpoint.
It masks protected spans and checks placeholder order, selected constraint terms, and numeric literals.
These checks cannot prove semantic equivalence.
Errors, timeouts, oversized inputs, and failed preservation checks deliver the Basic result.
Conch does not send a later cleanup response after fallback or cancellation.

Remote cleanup starts disabled.
Enabling remote cleanup requires confirming the HTTPS destination in Settings.
Only the masked transcript and a fixed cleanup instruction leave the device.
The transcript can still contain confidential information, and the provider may charge for requests.
API keys use the platform credential store and can be removed in Settings.
Plain HTTP is accepted only for loopback local endpoints.
Conch never falls back from a local provider to a remote provider.

Conch asks GitHub for the newest release's manifest shortly after launch and then every 24 hours.
The request carries no transcript, audio, setting, or identifier, and GitHub sees the device's IP address.
Settings turns the automatic check off, and the manual check stays available.
Conch installs an update only when you choose to, and only after verifying its signature against the public key built into the app.

Settings and vocabulary indexes use a local SQLite database.
Repository files supply paths and declared symbols without executing repository code.
Conch keeps audio and the latest result in memory.
Conch discards audio after success or cancellation.
Failed recognition retains bounded audio in memory for Retry until another recording, cancellation, or exit.
Conch writes no audio files.
Conch writes no transcript history unless you turn on Keep history in Settings, which is off by default.
With it on, Conch saves each delivered result and its raw transcript, with the delivery time, in the same local database, and keeps the newest 500.
The History page copies, searches, and deletes entries, and Clear history deletes all of them.
Turning Keep history off stops saving but leaves existing entries until you delete them.
Transcripts can contain confidential information, and the database file is not encrypted.
Conch keeps a diagnostic log, `conch.log`, in the same directory, whether or not Keep history is on. It holds about 4 MB of timestamped entries: stage times, delivery outcomes, failures, worker starts and exits, downloads, update checks, permission changes, and repository index sizes.
Entries never contain audio, transcripts, result text, or source text, and they show your home directory as `~`.
An inference worker's last lines of output are saved when it fails to start or exits unexpectedly. Conch cannot inspect that output, so read it on the Logs page before you share a log.
The Logs page copies, searches, and deletes entries, and Clear logs deletes all of them. The file is not encrypted.
Inference workers use authenticated loopback HTTP.
They stay running between recordings, stop after an idle period or on exit, and cancellation stops the ones the cancelled recording used.
