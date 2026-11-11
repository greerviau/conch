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

## Short dictations

The chunk measurement above covers a 120 s recording. A dictation shorter than about 9 s is a single tail, because a chunk needs 8 s of audio and a 1 s pause.
That tail was recognized in full after the stop, so it took the full request time: 0.6 s for a clip of about 5 s on `small.en` (`recognition-models.md` and the `default` rows below).
Two changes address it.

### Speculative recognition

Once speech is followed by 0.3 s of silence, `Chunker::settle` offers the audio since the last chunk and `Transcriber::speculate` recognizes it while the recording continues.
When the recording stops with no further speech, `Chunker::settled` lets the tail reuse that text.
"No further speech" means fewer than 100 ms of frames above the level that separated speech from silence when the audio was offered. The offer keeps that level, because a level recomputed over the longer recording can count quiet speech as silence and drop words.
A chunk or a later speculation queued behind an unstarted speculation discards it, and a failed speculation is recognized again by the tail.
The recognize closure of `Transcriber::start` receives `true` for a speculation and `false` for a chunk or tail, so a recognizer can treat speculative text as provisional.
Speculation and the stopwatch go through `Transcriber`, so neither assumes a particular recognizer or language.

`examples/latency.rs` measured it on the real `whisper-server` (Apple M1, Metal, `small.en`, warm worker, the 4.6 s synthetic clip followed by silence before the stop). Each row is one run; the two runs were about an hour apart on a machine that other builds were using:

| Silence before the stop | Recognized after the stop | Speculative |
| --- | --- | --- |
| 0.5 s | 0.30 s, 0.39 s | 0.24 s, 0.31 s (the speculation was still running at the stop) |
| 1.0 s | 0.25 s, 0.42 s | 0.00 s, 0.00 s (reused) |

These times already include the request settings below.
How long a user waits between the last word and the key release is not measured here.
The status line under the result now ends with the time from the stop to the text and the time of each stage (capture, recognition, preparation, cleanup, delivery), so the installed app reports it.
Stop-to-text in the installed app was not measured in this session, because an agent shell has no Accessibility trust for paste.

### Request settings for short audio

`scripts/benchmark-recognition.py` sends each setting to one warm `whisper-server` (voice detection on, 4 threads, no recognition hint).
The clips are 40 synthetic ones from four macOS `say` voices (Samantha, Daniel, Karen, Alex): 8 technical sentences of 4.4-6.0 s and 2 groups of three sentences of 15.2-17.0 s, 896 words in all.
Word error rate is the word-level edit distance after lowercasing and removing punctuation.
Time is the request time of the faster of two requests per clip, and the word error rate uses that request's text.
The identifiers (`useAuthStore`, `podGC`, `maxGroups`) have no hint, so the absolute error rates are higher than in `recognition-models.md`.
One run per cell; a difference of one point is about nine words and is within noise.

| Model | Setting | Word error rate | Mean time | Median time |
| --- | --- | --- | --- | --- |
| `small.en` | default | 12.2% | 713 ms | 599 ms |
| `small.en` | `temperature_inc=0` | 12.2% | 687 ms | 589 ms |
| `small.en` | + `no_timestamps` | 12.4% | 689 ms | 579 ms |
| `small.en` | + `audio_ctx` = clip frames + 25 | 26.7% | 403 ms | 257 ms |
| `small.en` | + `audio_ctx` = clip frames + 100 | 12.8% | 394 ms | 259 ms |
| `small.en` | + `audio_ctx` = clip frames + 200 | 12.4% | 389 ms | 281 ms |
| `base.en` | default | 14.0% | 249 ms | 207 ms |
| `base.en` | `temperature_inc=0` | 14.0% | 269 ms | 222 ms |
| `base.en` | + `no_timestamps` | 13.8% | 264 ms | 236 ms |
| `base.en` | + `audio_ctx` = clip frames + 25 | 108.6% | 280 ms | 126 ms |
| `base.en` | + `audio_ctx` = clip frames + 100 | 14.4% | 174 ms | 129 ms |
| `base.en` | + `audio_ctx` = clip frames + 200 | 14.2% | 174 ms | 129 ms |

An `audio_ctx` frame is 20 ms. Each row adds to the one above it, so `audio_ctx` rows include `temperature_inc=0` and `no_timestamps`.
`tiny.en` was not measured, because it is not installed in the application data directory.

- `temperature_inc=0` and `no_timestamps` alone change the time by less than the noise on this audio, because clean synthetic speech does not trigger the temperature fallback.
- `audio_ctx` roughly halves the median time of a short clip on `small.en` (599 to 281 ms) and cuts it by a third on `base.en` (207 to 129 ms).
- A tight window reduces accuracy. With a 0.5 s margin, `small.en` repeated the last two sentences of the 16.5 s Samantha clip and the 17.0 s Daniel clip of the same text (errors 2 to 59 per clip). `base.en` reached 108.6%; its clips were not read one by one. No clip repeated at margins of 1 s and more on `small.en`.
- With a 4 s margin, word error rate was within 0.2 points of the default on both models. The 4 s margin is `CONTEXT_MARGIN` in `inference.rs`.
- Margins of 1 s to 3 s measured 12.6% to 13.3% on `small.en` across three earlier runs, which is up to 1.1 points above the default. The extra errors came mostly from the short clips Karen-1 ("Addery try" for "Add a retry") and Daniel-2 ("isn't set" for "is unset") and the 15.7 s clip Daniel-8, which a one-run cell cannot separate from noise.
- Without `no_timestamps`, `audio_ctx` with the default temperature fallback (not shipped) gave 25.9% on one run and 14.2% on a rerun on `small.en`, probably because the fallback samples randomly; that was not isolated.
- Longer audio (over 30 s) keeps the defaults, because `no_timestamps` makes whisper.cpp advance by a whole 30 s window and a shorter `audio_ctx` silently drops audio beyond it (`src/whisper.cpp` at the checked-out revision, `whisper_full_with_state` and the mel copy in `whisper_encode_internal`).

Not measured: real human speech, real microphones and rooms, other languages, `language=auto`, the multilingual models, and `tiny.en`.
Rerun the table on recordings of your own voice with `--clips <folder>` (a `name.wav` and `name.txt` pair per clip) before relying on it. `--model` takes any Whisper model file, and `--language` sets the language sent with each request.
