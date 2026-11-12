# Recognition languages

This file records what was run for the recognition language setting (`docs/plans/2026-11-11-languages-and-recognition-engines.md`, work items 1 to 4).
It states only what was observed. Windows and Linux are untested beyond what `cargo clippy` and the unit tests cover on macOS.

## Method

Apple M1, 8 GiB RAM, Darwin 25.6.0.
`whisper-server` comes from `scripts/build-runtimes.py` (whisper.cpp v1.9.5, the revision the script pins) with Metal and 4 threads, as the app launches it: `--suppress-nst --vad` with the Silero model, and no `--language` flag.
The Whisper models are the pinned multilingual files from the Rust catalog, each checked against its SHA-256 after download.
Audio is synthetic speech from macOS `say` voices: Samantha (en), Thomas (fr), Anna (de), Monica (es), Luciana (pt), Alice (it), Milena (ru), and Kyoko (ja).
Synthetic speech tests the request plumbing and a rough detection rate. It does not test accents, other speakers, microphones, rooms, or code-switching by a person.
Each figure is one run. Decoding uses temperature 0. The language-lock test below was run three times and printed the same table each time; the detection tables were run once.
`scripts/measure-auto-detect.py` and `scripts/auto-detect-samples.json` regenerate the clips and the detection tables; `recognition-languages-detection.json` and `recognition-languages-lock.txt` hold the raw results behind the tables. The latency rows come from `src-tauri/examples/latency.rs`.

## Model language counts

`n_vocab` in each pinned ggml file's header decides how many language tokens the model has.
Read from the first bytes of each file at the pinned revision:

| File | n_vocab | Languages |
| --- | --- | --- |
| `ggml-tiny.en.bin` | 51864 | English only |
| `ggml-tiny.bin`, `ggml-base.bin`, `ggml-small.bin` | 51865 | 99 |
| `ggml-large-v3-turbo-q5_0.bin` | 51866 | 100 |

whisper.cpp computes `num_languages = n_vocab - 51765 - 1` for a multilingual model (`src/whisper.cpp`, `num_languages()`), and `whisper_token_lang` adds the language id to the start-of-transcript token.
Cantonese (`yue`) is id 99, the 100th language. On a 99-language model that id selects the translate token, so Cantonese works only on Large Turbo.
The first design in the spec listed all 100 languages for every multilingual model. The first review of work item 2 found the error, and the catalog now lists 99 for Tiny, Base, and Small.

## Request plumbing against the real worker

`the_chosen_language_reaches_the_multilingual_recognizer_on_each_request` in `src-tauri/tests/acceptance.rs` (`#[ignore]`) sent one `say` sentence each in English, French, German, and Spanish to multilingual Tiny on CPU.
One pooled `whisper-server` process served the languages `en`, `fr`, `de`, `es`, and `auto` in turn, and each request returned the sentence with a word overlap of 1.00 against the spoken text.
This confirms that the pinned server reads `language` on each request, so a language change does not restart the worker.

## Auto-detect

### Method

101 clips: 48 sentences of 1.6 to 4.9 s (six per language), 48 short phrases of 0.5 to 1.6 s (six per language), and 5 sentences that mix prose with the identifiers `podGC` and `maxGroups`.
The 48 + 48 clips are the non-English and English sentences and phrases; the five mixed clips are in French, German, Spanish, Portuguese, and Italian.
Every clip was sent with `language=auto` and `response_format=verbose_json`, with and without the recognition hint `Vocabulary: podGC, maxGroups.`, in three request variants:

- **default**: no `audio_ctx` (the full 30 s encoder window). This is how the app sends a request today.
- **short-audio**: `audio_ctx` = clip frames + 200, `no_timestamps`, `temperature_inc` 0, the values the first version of PR #26 (df85024) sent for clips up to 30 s, in clip order on one worker. PR #26 removed those fields in fd3f933, so the app does not send them today.
- **own-window**: the short-audio fields, with each clip sent twice and the second answer kept (see the next paragraph).

whisper.cpp v1.9.5 detects the language before it applies the request's `audio_ctx` (`whisper_full_with_state` in `src/whisper.cpp`: detection near line 7003, `state->exp_n_audio_ctx = params.audio_ctx` at line 7146).
A request therefore detects with the encoder window the previous request on the same worker left behind.
In the short-audio variant each clip is detected with the previous clip's window, as the app would have it. In the own-window variant the first request leaves the clip's own window for the second to detect with.
Each variant of each model ran on its own fresh worker so one variant's window cannot reach another.
An earlier run of this measurement interleaved the variants on one worker. Its detection counts mixed the windows of neighboring requests, so it was discarded, and the conclusions below come from the rerun.
The run recorded 1,818 results (3 models, 6 variants, 101 clips) from 2,424 requests, because the own-window variants send each clip twice. `recognition-languages-detection.json` holds every result.

### Detection rate

Clips whose detected language was the spoken language. The hint never changed a detected language (see the next section), so the table shows the runs without it.

| Model | Request | Mixed (5) | Phrases (48) | Sentences (48) |
|---|---|---|---|---|
| tiny | default | 5 | 35 | 48 |
| tiny | short-audio | 5 | 28 | 46 |
| tiny | own-window | 5 | 32 | 47 |
| base | default | 5 | 38 | 48 |
| base | short-audio | 3 | 28 | 40 |
| base | own-window | 4 | 27 | 45 |
| small | default | 5 | 38 | 48 |
| small | short-audio | 5 | 35 | 48 |
| small | own-window | 5 | 37 | 48 |

With the default request, sentences are detected reliably (48 of 48 on every model) and short phrases are not: Tiny 35, Base 38, and Small 38 of 48.
Phrases by length, default request (correct out of total):

| Model | Under 1.0 s | 1.0 to 1.3 s | 1.3 s or longer |
| --- | --- | --- | --- |
| tiny | 22/32 | 9/12 | 4/4 |
| base | 23/32 | 11/12 | 4/4 |
| small | 25/32 | 9/12 | 4/4 |

Phrases by language, default request (correct out of 6):

| Model | de | en | es | fr | it | ja | pt | ru |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tiny | 2 | 6 | 4 | 5 | 3 | 6 | 5 | 4 |
| base | 2 | 6 | 5 | 6 | 1 | 6 | 6 | 6 |
| small | 2 | 6 | 5 | 5 | 4 | 6 | 5 | 5 |

German and Italian phrases fail most. A misdetected phrase is recognized in the wrong language, which usually looks like a translation or unrelated text: Base returned "Data detests." for German "Starte die Tests" and Small returned "Launch the test." for French "lance les tests".
Detected as English, out of the 84 non-English sentences and phrases (the 5 mixed clips are excluded):

| Model | Request | Detected as English | Out of |
|---|---|---|---|
| tiny | default | 9 | 84 |
| tiny | short-audio | 13 | 84 |
| tiny | own-window | 8 | 84 |
| base | default | 7 | 84 |
| base | short-audio | 11 | 84 |
| base | own-window | 10 | 84 |
| small | default | 7 | 84 |
| small | short-audio | 6 | 84 |
| small | own-window | 7 | 84 |

### The recognition hint and Auto-detect

The hint did not change the detected language of any clip: 0 of the 909 clips differed with and without it, in every model and variant.
That follows from the order in whisper.cpp: the language is detected from the audio before the prompt is applied.
So the English identifier hint cannot push detection toward English. It changed the text of 524 of the 909 clips, which is what it is for. With Small and the default request, the five mixed sentences came back as follows without and with the hint:

| Language | Without the hint | With the hint |
| --- | --- | --- |
| de | Dateipot GC, Max Groops | DateipodGC, maxGroups |
| es | por GC, MaxGrowlps | podGC, maxGroups |
| fr | PauGC, maxGroups | podGC, maxGroups |
| it | pod gc, max groups | podGC, maxGroups |
| pt | PODGC, maxGroups with an extra letter ("maxGropes") | podGC, maxGroups |

So Conch keeps sending the hint when the language is Auto-detect. The prose stayed in its own language in all five.
The German sentence with the hint joined the file name to the identifier ("DateipodGC"), a recognition error the hint does not fix.

### Interaction with the short-audio request settings

The first version of the latency work (PR #26, df85024) sent `audio_ctx`, `no_timestamps`, and `temperature_inc` for audio up to 30 s. Its later commit (fd3f933) removed them after a per-request `audio_ctx` made later requests on the same worker lose text (`docs/validation/latency.md` on that branch). This section records what the settings did to detection, for any change that sends a narrowed window again. A smaller encoder window lowers detection quality on the smaller models:

- Base: sentences fell from 48 to 40 of 48 (short-audio) and 45 (own-window), phrases from 38 to 28 and 27, mixed clips from 5 to 3 and 4.
- Tiny: sentences fell from 48 to 46 and 47, phrases from 35 to 28 and 32.
- Small: sentences stayed at 48 of 48, phrases fell from 38 to 35 and 37.

The loss does not come from cutting off speech: the window is the clip length plus 4 s, so it covers the clip. The models were trained on full 30 s windows, and detection from a shorter window is less reliable.
Because the window carries over between requests, even a request that sends no `audio_ctx` detects with a short window when the previous request sent one.
Small, the default multilingual model, loses little (3 of 48 phrases in the app-like variant). Tiny and Base lose 1 to 8 sentences and 3 to 11 phrases.

Consequence if a narrowed window is sent again: while the configured language is Auto-detect, no request of the recording should carry it, including the chunks sent with the locked language, because the last chunk's window is what the next recording's first detection would use. The rule follows the configured language, not the language a request carries, and a worker that served requests with short windows must restart when the setting moves to Auto-detect. PR #26 sends no such window, and `a_request_never_overrides_the_window_or_the_decoding_of_the_recognizer` in `inference.rs` guards that for every language, so this branch carries no Auto-detect rule for it.
The speculative recognition with Auto-detect was not run; only the request settings were measured.

### Flips between chunks

A recording here is a sentence (the first chunk) followed by a short phrase of the same language (the tail after the last pause). A flip is a tail whose detected language differs from the first chunk's.

| Model | Request | Recordings | Flips | First chunk right, tail wrong |
|---|---|---|---|---|
| tiny | default | 48 | 13 | 13 |
| tiny | short-audio | 48 | 21 | 19 |
| tiny | own-window | 48 | 17 | 16 |
| base | default | 48 | 10 | 10 |
| base | short-audio | 48 | 23 | 16 |
| base | own-window | 48 | 23 | 20 |
| small | default | 48 | 10 | 10 |
| small | short-audio | 48 | 13 | 13 |
| small | own-window | 48 | 11 | 11 |

Flips occurred in every variant, 10 to 23 of 48 recordings. In most of them (at least 16 of 23 in the worst variant) the first chunk was right and the tail wrong. That meets the spec's condition for reusing the first chunk's language.

### Language lock

`ChunkRecognizer` (`src-tauri/src/inference.rs`) recognizes the first final chunk of a recording that has text with `auto`, reads the detected language from the response, and sends that language with every later chunk of the same recording. A chosen language is never replaced. The whole-recording fallback in `process` still sends `auto` once. Its `transcribe_speculative` method uses the language but never sets it, for the speculative recognition on the latency branch.
On that branch one closure serves chunks, speculations, and tails, so after the merge a speculation on a short start of a chunk could set the language from audio too short to detect from. The latency thread has made the closure take a flag for speculations (`Fn(Vec<u8>, bool)`), and the merge wires it to `transcribe_speculative`. Until then no code path calls it.
Unit tests with a fake worker cover the request fields and the lock sequence: a chunk without text, with only whitespace, with a language outside the table, or that failed does not lock; a second chunk reuses the language; a speculative call never locks; a new recording detects again; a chosen language is never replaced.

`the_first_chunks_language_keeps_a_short_tail_in_that_language` (`#[ignore]`, multilingual Small, Metal, default request) recognized the same 48 recordings twice: tails after the lock, and tails alone with fresh detection.
Mean word overlap of the tail with the spoken phrase: 0.920 with the lock and 0.778 with detection per chunk (`recognition-languages-lock.txt` has every recording; the table sums it):

| Language | Recordings | Tail overlap with the lock | Tail overlap with detection per chunk |
|---|---|---|---|
| en | 6 | 1.00 | 1.00 |
| fr | 6 | 1.00 | 0.83 |
| de | 6 | 0.89 | 0.33 |
| es | 6 | 1.00 | 0.83 |
| pt | 6 | 1.00 | 0.92 |
| it | 6 | 0.56 | 0.56 |
| ru | 6 | 0.92 | 0.75 |
| ja | 6 | 1.00 | 1.00 |
| all | 48 | 0.920 | 0.778 |

Cases the lock fixed include German "Oeffne die Datei" (Arabic script before, correct German after), "Vielen Dank" ("Feel and Dunk." before), and French "Lance les tests" ("Launch the test." before).
Italian did not improve (0.56 in both columns): its tails were recognized poorly in Italian too, so the lock does not fix a recognition error that is not a detection error.
The test ran with the default request, which is the request the app sends at PR #26 fd3f933.

Limits:

- A misdetected first chunk sets the wrong language for the whole recording. Short first chunks are more susceptible.
- A recording that is only one short chunk has no earlier chunk to rely on.
- The lock changes the language a recording uses after its first chunk, so a person who switches language mid-recording gets the first language throughout. Start a new recording when switching languages.

### Silence and noise

`multilingual_models_return_no_text_for_silence_and_noise` (`#[ignore]`) sent 1 s of silence and 3 s of synthetic noise (the noise the `.en` acceptance test uses) to multilingual Tiny, Base, and Small, with the languages `auto`, `fr`, and `en`, each with and without the hint.
All 36 requests returned empty text. The worker starts with voice detection, so the multilingual models answered audio without speech as the `.en` models do.
This covers silence and one noise signal. Room noise from a real microphone and speech-like noise were not tried.

## Latency and memory of the multilingual models

`src-tauri/examples/latency.rs` on a 60 s recording (the repeated synthetic clip in `.runtime/synthetic.wav`), language `en`, Metal, voice detection on, with the pipeline as it stands in this branch.
Two runs per model. Peak memory is the `maximum resident set size` that `/usr/bin/time -l` reports, the largest process in the run (the worker).
The machine ran other work during these runs, and the two runs of one model differ by up to 2 s for a single request, so treat the figures as an order of magnitude.

| Model | Worker start (s) | One request after stop (s) | Chunked, stop to text (s) | Peak memory (MB) |
| --- | --- | --- | --- | --- |
| tiny.en | 0.17, 0.48 | 1.08, 2.16 | 0.20, 0.21 | 258, 290 |
| tiny (multilingual) | 0.41, 0.16 | 1.14, 2.61 | 0.31, 0.23 | 263, 262 |
| base (multilingual) | 0.87, 0.23 | 3.02, 3.13 | 0.34, 0.37 | 314, 356 |
| small (multilingual) | 0.83, 0.94 | 5.23, 4.64 | 0.89, 1.01 | 719, 734 |

`small.en` and `large-v3-turbo` were not run: their files were not downloaded for this record. Parakeet is out of scope here.

## Not measured

- Real human speech, accents, microphones, and rooms.
- Per-language word error rates. Whisper's published error rates differ widely between languages, and Conch makes no per-language accuracy claim.
- Mixed speech beyond 5 synthetic sentences.
- The speculative recognition path with Auto-detect, and the language lock with a narrowed encoder window.
- Large Turbo, including Cantonese on it.
- Windows and Linux.
