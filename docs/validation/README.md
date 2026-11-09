# Conch validation

## Environment and inference experiment

The experiment runs on Apple M1, 8 GiB RAM, macOS 26.6.2, Rust 1.95.0, and Node 26.8.1.
The worker sources are pinned in `scripts/build-runtimes.py`.
The model URLs, revisions, sizes, and SHA-256 values are pinned in the Rust model manifest.
Whisper tiny.en supplies the initial English recognizer.
S1-mini q4_k_m by Superwhisper supplies optional Enhanced cleanup.
The app sends the published S1-mini system prompt and semi-formal/prose/general controls, disables thinking, and uses greedy decoding.

`synthetic-inference.json` records isolated warm inference requests on CPU and the default macOS acceleration path.
Those measurements exclude worker startup, recording, indexing, download time, and desktop delivery.
They do not establish stop-to-result latency or recognition quality on human speech.
The synthetic audio uses macOS speech synthesis and contains a fixed technical instruction.
Whisper preserves the tested negation and numbers but produces casing differences in technical names.
The S1-mini fixtures preserve the tested placeholders and return empty output for hesitation-only input.
The self-correction fixture removes a phrase containing "no", which the app's conservative constraint check rejects and replaces with Basic output.

No representative recordings, approved expected-output corpus, or target-hardware performance thresholds have been supplied.
Identifier error rate, unwanted substitution rate, manual edit count, peak process memory, long-input model quality, and human voice quality remain unmeasured.
The runtime/model selection remains provisional pending that corpus.

## Resource limits

- Recordings contain at most 180 seconds of mono 16 kHz samples in memory.
- Capture stores at most 2,880,000 floats, followed by a 16-bit WAV for recognition.
- Recognition startup waits at most 45 seconds, and its request times out after 90 seconds.
- Enhanced startup waits at most 45 seconds, and its request times out after 30 seconds.
- Enhanced input contains at most 3,000 bytes before tokenization, with a 4,096-token managed context and 1,024-token output limit.
- Cleanup responses are read through a 64 KiB limit.
- Indexing skips files over 512 KiB and caps each completed repository index at 50,000 entries.
- Repository refreshes run outside recording and publish completed indexes to SQLite.
- File watchers request refreshes with a 500 ms debounce; a 30-second scan also checks configuration and root availability.

The byte limit is conservative and does not establish the model's exact token count.
The app uses Basic on longer input.
Index status reports incomplete scans and skipped symbol extraction.

## Automated acceptance coverage

- Exact mappings, longest-span matching, nonrecursive replacement, repository override, and retained conflicts.
- Basic and Off cleanup, quoted hesitation tokens, exact mapping output, repository literals, negation, numbers, and repetition.
- Repository suggestions that leave delivered text unchanged, accepted edits, and undo after manual changes elsewhere.
- Ignore rules, extra exclusions, binary files, Python/TypeScript declarations, source attribution, refresh, and external symlinks.
- Immutable mapping snapshots, repository selection isolation, removal, and SQLite persistence without transcript/audio files.
- Push-to-talk, toggle, repeat suppression, processing exclusion, cancellation, and stale-result rejection.
- Single-copy/single-paste delivery decisions, changed focus, missing eligible focus, empty output, and failed paste without retry.
- Endpoint transport restrictions, disabled remote processing, masked local endpoint payloads, corrupt placeholders, and changed constraints.
- An opt-in integration test starts both bundled workers and runs synthetic WAV recognition and managed cleanup.

`red-runs.md` records the initial failing assertions for behavior developed through the test harness.
Some persistence, audio, UI, and integration code was implemented before its acceptance tests; the implementation is not entirely test-first.

## Remaining acceptance work

- Run a representative, user-approved recording corpus and measure errors, edit counts, cold stop-to-result latency, and peak memory.
- Validate microphone capture and permissions with human dictation.
- Validate hotkey conflicts and both activation modes in receiving apps.
- Validate macOS eligible-field detection, selection replacement, secure fields, focus races, permission denial, and failed insertion in TextEdit, browsers, and T3.
- Build and exercise Windows UI Automation, credential storage, capture, shortcuts, and package installation.
- Build and exercise Linux separately on named X11 and Wayland desktops, including clipboard ownership after the write handle closes.
- Validate remote HTTPS cleanup, authentication failures, credential removal, timeouts, and cancellation against a configured service.
- Validate fresh installation, model downloads, offline operation with OS networking disabled, signing, and installation on each release target.

The local worker integration test runs without contacting a remote model endpoint after setup.
Networking is not disabled at the operating-system level during that test.
Cross-platform configuration does not establish cross-platform test success.
