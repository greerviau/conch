# Recognition models and voice detection

## Method

Apple M1, 8 GiB RAM, whisper.cpp `whisper-server` from `scripts/build-runtimes.py` with Metal, 4 threads, `--language en --suppress-nst`.
Models come from the pinned revision in the Rust manifest.
Audio is synthetic: 8 technical dictation sentences read by four macOS `say` voices (Samantha, Daniel, Karen, Alex), 32 clips of 4.8-6.3 s (mean 5.4 s) and 572 words.
Word error rate is the word-level edit distance to the reference sentence after lowercasing and removing punctuation.
Each figure is one run against a warm worker.
Throwaway scripts that call `whisper-server` directly produced them. Those scripts are not in the repository, so these numbers cannot be rerun from it.
Real human speech, real microphones, and real rooms are not measured here, so these figures rank models and show failure modes but do not predict a user's error rate.

## Accuracy and time by model

Greedy decoding is the app's setting. Beam size 5 is shown for comparison and is not enabled.

| Model | Word error rate | Word error rate, beam 5 | Seconds per clip | Seconds per clip, beam 5 |
| --- | --- | --- | --- | --- |
| tiny.en | 5.4% | 5.2% | 0.20 | 0.29 |
| base.en | 4.2% | 3.8% | 0.29 | 0.45 |
| small.en | 3.1% | 2.8% | 0.64 | 0.72 |

Time is the mean request time for a whole clip, excluding worker startup.
A 120 s recording of repeated synthetic speech took about 28 ms per second of audio on tiny.en (see `latency.md`), and these figures suggest small.en takes about three times as long.
The time after you stop is the time for the last chunk, which is at most 25 s of audio.
That estimate for small.en is unmeasured in the app.

The fallback that recognizes a whole recording in one request has a 90 s timeout.
A 180 s recording of the same clips, with voice detection and a hint, took 6.1 s on tiny.en with Metal and 4.0 s with `--no-gpu`, and 10.1 s on small.en with Metal and 13.7 s with `--no-gpu`.
On that input tiny.en returned 597 words and small.en 535 words; the clips contain about 545 words in 180 s, and the extra tiny.en words were not examined.

## Silence and noise after speech

With the recognition hint `Vocabulary: podGC, maxGroups, TaskRunner, useAuthStore, conch_worker.` and no voice detection, the recognizer answered audio without speech:

| Audio | tiny.en | small.en |
| --- | --- | --- |
| 1 s silence | `you` | `You can find it on the screen.` |
| 3 s synthetic noise | `Thank you.` | `Thank you for watching!` |
| click then noise | `You` | `Thank you for watching!` |

Without the hint, tiny.en returned an empty string for the same audio.
With `--vad --vad-model ggml-silero-v5.1.2.bin`, both models returned an empty string for all of it.
Speech followed by 1-2 s of noise transcribed the same with and without voice detection.
The noise was Gaussian or a decaying click, not a recorded room.

Voice detection did not drop short isolated words.
Seven words (No, Yes, Stop, Undo, Commit, Okay, Go) by two voices produced no empty output with the default 250 ms minimum speech length or with 100 ms.
The misrecognitions in that test (for example "Commit" heard as "Comment.") also occurred without voice detection.

`acceptance.rs` repeats the silence check with the real worker: 3 s of low-level noise and 1 s of digital silence with the hint return an empty string on tiny.en.

## Noise suppression, not shipped

FFmpeg `afftdn` and `arnndn` (RNNoise `sh.rnnn`) were applied to the clips mixed with pink noise or babble (three other voices) at 10 dB and 0 dB signal-to-noise ratio, with voice detection and the hint on.
Neither helped consistently. On small.en at pink noise 0 dB, word error rate was 15.2% unprocessed, 15.6% with `afftdn`, and 21.5% with `arnndn`.
On tiny.en at pink noise 0 dB, `afftdn` reduced it from 50.3% to 33.4% while `arnndn` raised it to 60.3%.
Both raised it at babble 0 dB for small.en.
Small.en without denoising scored below tiny.en with its best denoiser in all four noisy conditions: 4.2%, 3.3%, 15.2%, and 57.2% for small.en against 11.0%, 10.3%, 33.4%, and 82.5% for tiny.en (pink 10 dB, babble 10 dB, pink 0 dB, babble 0 dB).
The clean small.en rate with the hint and voice detection was 1.7%, against 3.1% without them in the table above; the two effects were not separated.

## Limits

- No human recordings, so none of these figures describe real dictation.
- One run per cell. Differences of about one percentage point on 572 words are within noise.
- Peak memory of small.en with S1-mini loaded is unmeasured.
- Stop-to-text latency with small.en in the app is unmeasured.
