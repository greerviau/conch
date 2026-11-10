# Dictation latency

## Cause

The earlier flow started a new `whisper-server` for every recording and a new `llama-server` for every Enhanced cleanup, then stopped it.
`synthetic-inference.json` records a 0.36-0.54 s recognizer startup and a 1.7-2.6 s S1-mini startup on the validation Mac.
The recognizer then processed the whole recording after the user stopped.
The interface polled the `view` command every 700 ms, and each call parsed every stored vocabulary index while holding the lock that recording and delivery use.

## Changes

- The recognizer starts when recording starts, and managed cleanup starts then when Enhanced is selected.
- Workers stay running between recordings and stop after an idle period.
- Chunks recognized during recording leave only the final chunk for after the stop.
- `view` reads index status without parsing vocabulary entries and no longer holds the shared lock while reading the database.
- Suggestions are computed once per result unless Enhanced cleanup changes the text. Auto-accept runs on the suggestions of the final text.

## Measurements

Apple M1, whisper.cpp tiny.en with Metal, 120 seconds of a repeated synthetic clip with pauses.
`examples/latency.rs` produced these numbers.
The audio is synthetic and repeated, so recognition quality on real speech is not measured here.

| Path | Time from stop to text |
| --- | --- |
| One request after stop (earlier behavior, warm worker) | 2.4-3.2 s |
| Chunks recognized during recording | 0.2 s |

Both paths recognized all 22 repetitions of the phrase.
A warm 120 s request takes about 28 ms per second of audio, so the single-request time grows with recording length and the chunked time does not.
Worker startup measured 0.29 s in one run and 0.49 s and 7.5 s in two others on the same machine, which is why startup now overlaps recording.

Enhanced cleanup time after a warm start was not measured in the application.
The recorded synthetic runs show 0.04-0.27 s of inference for short inputs on Metal; longer inputs scale with output length.
