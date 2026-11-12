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
Speculative recognition addresses it. A second change, per-request `whisper-server` settings, was shipped and then removed because it made later requests lose text (see the last section).

### Speculative recognition

Once speech is followed by 0.3 s of silence, `Chunker::settle` offers the audio since the last chunk and `Transcriber::speculate` recognizes it while the recording continues.
When the recording stops with no further speech, `Chunker::settled` lets the tail reuse that text.
"No further speech" means fewer than 100 ms of frames above the level that separated speech from silence when the audio was offered. The offer keeps that level, because a level recomputed over the longer recording can count quiet speech as silence and drop words.
A chunk or a later speculation queued behind an unstarted speculation discards it, and a failed speculation is recognized again by the tail.
The recognize closure of `Transcriber::start` receives `true` for a speculation and `false` for a chunk or tail, so a recognizer can treat speculative text as provisional.
Speculation and the stopwatch go through `Transcriber`, so neither assumes a particular recognizer or language.

`examples/latency.rs` measured it on the real `whisper-server` (Apple M1, Metal, `small.en`, warm worker, a 4.5 s synthetic clip followed by silence before the stop, default request settings). One run per row:

| Silence before the stop | Recognized after the stop | Speculative |
| --- | --- | --- |
| 0.5 s | 0.56 s | 0.32 s (the speculation was still running at the stop) |
| 1.0 s | 0.57 s | 0.00 s (reused) |

How long a user waits between the last word and the key release is not measured here.
The status line under the result now ends with the time from the stop to the text and the time of each stage (capture, recognition, preparation, cleanup, delivery), so the installed app reports it.
Stop-to-text in the installed app was not measured in this session, because an agent shell has no Accessibility trust for paste.

### Request settings for short audio (removed)

Sending `audio_ctx` sized to the clip, `no_timestamps` and `temperature_inc=0` with each request cut the request time of a short clip by half on `small.en` (table below), and it was shipped in the first version of this change.
It made a later request on the same worker lose text. On a fresh `small.en` worker, `Daniel-2s`, `Daniel-3s` and `Daniel-4s` (11, 16 and 24 s of synthetic speech) sent in that order, each with its own `audio_ctx` (759, 1008, 1390), returned 6 of the 65 words for the third clip ("The pod gc setting is unsaid"). The same clip alone, or after only the first clip, returned all 65 words, and with the default settings in the same order all were correct.
Over 190 requests in random order (96 clips of 4 to 30 s, two passes, one fresh worker per setting), 6 requests lost more than 15% of their words with per-request `audio_ctx`, all on 22 to 24 s clips, and the word error rate was 16.6%. The default settings, `no_timestamps` with `temperature_inc=0`, `audio_ctx` rounded up to 250-frame steps, and `audio_ctx` only for clips up to 10 s lost none (11.5%, 11.1%, 11.3% and 11.2%).
Through the recording flow (`examples/dictation_check.rs`: chunker, transcriber and real worker, each dictation followed by 0.3 to 1.2 s of quiet), 96 dictations lost words 15 times with per-request `audio_ctx` and no speculation, and the word error rate was 22.0%. With speculation on, 2 of 96 lost words, one by dropping the last sentence. With the request settings removed, 192 dictations (two passes) lost words 0 times and the word error rate was 11.3%.
The cause inside `whisper.cpp` was not found. The loss depends on the requests that came before, so it is not a property of one request's `audio_ctx`; the benchmark below sent clips in one fixed order and did not show it. The 0.5 s margin failures and the 25.9% / 14.2% rerun noted below were probably the same effect, not temperature sampling as first assumed.
Speculation is not the cause: with speculation on and `audio_ctx` off, 96 dictations lost words 0 times.
The request settings are removed. Per-request `audio_ctx` could return only after the cause is found and a run like `dictation_check` loses no text.
What follows is the measurement that shipped them, kept because it shows the speed available.

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

## Enhanced cleanup during the recording

After the request settings were removed, Enhanced cleanup was the largest stage after the stop: 436 ms and 909 ms in two status lines from the installed app (71% to 72% of 603 ms and 1279 ms).
The managed S1-mini worker (`llama-server`, Apple M1) generates the cleaned text token by token at about 9.6 ms per token, and the time grows linearly with it:

| Output tokens | Cleanup request |
| --- | --- |
| 25 | 248 ms |
| 59 | 576 ms |
| 79 | 750 ms |
| 110 | 1112 ms |

The prompt is cached after the first request (1 token evaluated, about 10 ms) and `/tokenize` takes 1 to 2 ms, so the time is the generation of the rewritten text.
About 100 tokens per second on the GPU and 85 on the CPU (`--gpu-layers 0`) suggest that decoding is limited by memory bandwidth, so the worker's threads and layers are not a lever (moderate confidence).

Conch now cleans each chunk and the tail as soon as it is recognized (`Transcriber::start_cleaning`, a second thread behind the recognizer).
After the stop only the tail's cleaning is left, unless the cleaner is slower than the speech: parts are cleaned one at a time, so earlier parts can still be queued at the stop.
A failure of any part gives Basic cleanup for the whole result.
Each part is masked alone, so a mapping or a quotation that spans two parts is not protected in either; `core::parts_mask_like_whole` detects that and the whole text is cleaned after the stop instead.
A recognition failure drops the parts still queued for cleaning and does not wait for the one in progress, so the recording can be recognized again at once.
`examples/cleanup_latency.rs` fed the 16 long synthetic dictations (10 to 30 s, 0.4 to 0.9 s pauses between sentences, 0.6 s of quiet before the stop, `small.en`, managed S1-mini) at real time, once with per-chunk cleaning and once with the earlier flow (`after`: one cleanup of the whole text after the stop). One run per clip.

| Dictation | Chunks | Per-chunk cleaning, stop to text | One cleanup after the stop |
| --- | --- | --- | --- |
| `Alex-5s` (29 s) | 1 | 543 ms | 1307 ms |
| `Daniel-5s` (30 s) | 1 | 801 ms | 1433 ms |
| `Karen-5s` (29 s) | 1 | 628 ms | 1322 ms |
| `Samantha-5s` (27 s) | 1 | 410 ms | 1145 ms |
| 12 clips of 10 to 24 s | 0 | 816 to 1926 ms (median 1410 ms) | 696 to 1938 ms (median 1257 ms) |

- A dictation with a chunk saves about 0.6 to 0.8 s: the wait is the tail's recognition and its cleaning, about 170 to 370 ms and 240 to 440 ms in these runs.
- A dictation without a chunk gains nothing, and the two columns differ only by run-to-run variation. A chunk needs 8 s of audio and a pause of 1 s (or 25 s of audio pending), and the pauses between the sentences of these clips are shorter, so only the five-sentence clips had one. Recognition of the 10 to 24 s tail (0.3 to 1.0 s) now dominates those dictations.
- The cleaned text of the per-chunk run equalled the whole-text cleanup of the same raw text in 16 of 16 dictations, but 12 of them had a single part, so only the 4 dictations with a chunk compare per-chunk with whole-text cleaning; their word error rate against the spoken words was the same (12.1% over all 16).
- The `cleanup_latency` run without `after` also cleans the whole text once after the stop to compare texts. That request reuses the prompt cache of the per-chunk request, so its time (356 to 1039 ms) is too low for the earlier flow; the `after` run is the comparison.
- Four requests at once on `llama-server --parallel 4` took 481 ms for four 21 to 25 token sentences against 857 ms for the 92-token text in one request (1.8 times faster, not 4 times). The worker runs one slot, where the four requests took 943 ms. This is not used.

Not measured: real speech and pauses, other cleanup providers (a remote endpoint adds a round trip per part), the status-line stage times in the installed app, and whether a chunk cleaned without its neighbours reads worse on real text than the whole text cleaned at once.
