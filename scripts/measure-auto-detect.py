#!/usr/bin/env python3
"""Measure how reliably whisper-server detects the spoken language with language=auto.

Two steps, both stdlib only:

  generate   Speak the sentences in auto-detect-samples.json with macOS `say` voices and
             write 16 kHz mono WAV files. Synthetic speech tests the request plumbing and a
             rough detection rate. It does not test accents, code-switching, or other speakers.
  measure    For each model and each variant, start a fresh whisper-server and send every
             clip with language=auto. The variants are the default request, the short-audio
             settings the app sends for clips up to 30 s, and the short-audio settings sent
             twice per clip (see below), each with and without the recognition hint. Print
             markdown tables and write every result to a JSON file.

whisper.cpp (v1.9.5) detects the language before it applies the request's audio_ctx, so a
request detects with the encoder window the previous request on the same worker left behind.
The "short-audio" variant keeps that behavior, as the app has it: each clip is detected with
the previous clip's window. The "own-window" variant sends each clip twice with the same
short-audio fields and keeps the second answer, so the detection uses the clip's own window.
Every variant runs on its own fresh worker so no variant sees another variant's window.

A "recording" pairs a sentence (the first chunk) with a short phrase of the same language
(the tail after the last pause). A flip is a tail whose detected language differs from the
detected language of its first chunk.

Example:
  python3 scripts/measure-auto-detect.py generate /tmp/auto-detect-clips
  CONCH_WORKER_TOKEN is set by the script itself.
  python3 scripts/measure-auto-detect.py measure --workers src-tauri/workers \\
      --models /tmp/models --clips /tmp/auto-detect-clips --names tiny base small \\
      --output /tmp/auto-detect-results.json
"""

import argparse
import collections
import http.client
import json
import os
import secrets
import socket
import subprocess
import sys
import time
import uuid
import wave

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLES = os.path.join(HERE, "auto-detect-samples.json")
# whisper.cpp reports the language by its lower-case English name.
CODES = {
    "english": "en",
    "french": "fr",
    "german": "de",
    "spanish": "es",
    "portuguese": "pt",
    "italian": "it",
    "russian": "ru",
    "japanese": "ja",
}
SILERO = "ggml-silero-v5.1.2.bin"
# The app sizes the encoder window to the clip for audio up to this long.
# Keep in step with `whisper_settings` in src-tauri/src/inference.rs.
SHORT_AUDIO_SECONDS = 30.0
CONTEXT_MARGIN = 200


def load_samples():
    with open(SAMPLES, encoding="utf-8") as handle:
        return json.load(handle)


def clips(samples):
    """Every clip as (kind, language, index, text)."""
    found = []
    for kind in ("sentences", "phrases", "mixed"):
        for language, texts in samples[kind].items():
            for index, text in enumerate(texts):
                found.append((kind.rstrip("s") if kind != "mixed" else "mixed", language, index, text))
    return found


def clip_name(kind, language, index):
    return f"{kind}-{language}-{index}.wav"


def generate(args):
    samples = load_samples()
    os.makedirs(args.out, exist_ok=True)
    for kind, language, index, text in clips(samples):
        voice = samples["voices"][language]
        target = os.path.join(args.out, clip_name(kind, language, index))
        aiff = target + ".aiff"
        subprocess.run(["say", "-v", voice, "-o", aiff, text], check=True)
        subprocess.run(
            ["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1", aiff, target],
            check=True,
        )
        os.remove(aiff)
    print(f"wrote {len(clips(samples))} clips to {args.out}")


def seconds_of(path):
    with wave.open(path) as clip:
        return clip.getnframes() / clip.getframerate()


def short_audio_fields(seconds):
    if seconds > SHORT_AUDIO_SECONDS:
        return {}
    frames = min(int(-(-seconds * 50 // 1)) + CONTEXT_MARGIN, 1500)
    return {"audio_ctx": str(frames), "no_timestamps": "true", "temperature_inc": "0"}


def multipart(fields, wav):
    boundary = uuid.uuid4().hex
    parts = []
    for name, value in fields.items():
        parts.append(
            f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
        )
    parts.append(
        (
            f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="recording.wav"\r\n'
            "Content-Type: audio/wav\r\n\r\n"
        ).encode()
        + wav
        + b"\r\n"
    )
    parts.append(f"--{boundary}--\r\n".encode())
    return boundary, b"".join(parts)


class Server:
    def __init__(self, workers, model, vad):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            self.port = probe.getsockname()[1]
        self.token = secrets.token_hex(16)
        env = dict(os.environ, CONCH_WORKER_TOKEN=self.token)
        self.process = subprocess.Popen(
            [
                os.path.join(workers, "whisper-server"),
                "--host", "127.0.0.1",
                "--port", str(self.port),
                "--model", model,
                "--threads", "4",
                "--suppress-nst",
                "--vad",
                "--vad-model", vad,
            ],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        started = time.time()
        while time.time() - started < 60:
            try:
                connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=1)
                connection.request("GET", "/health", headers={"Authorization": f"Bearer {self.token}"})
                if connection.getresponse().status == 200:
                    return
            except OSError:
                time.sleep(0.1)
        raise RuntimeError("whisper-server did not start")

    def detect(self, wav, fields):
        boundary, body = multipart(fields, wav)
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=120)
        connection.request(
            "POST",
            "/inference",
            body=body,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Content-Type": f"multipart/form-data; boundary={boundary}",
            },
        )
        reply = connection.getresponse()
        data = json.loads(reply.read())
        if reply.status != 200:
            raise RuntimeError(f"inference failed: {data}")
        return data

    def stop(self):
        self.process.kill()
        self.process.wait()


def measure(args):
    samples = load_samples()
    vad = os.path.join(args.models, SILERO)
    rows = []
    variants = [
        (settings, hinted)
        for settings in ("default", "short-audio", "own-window")
        for hinted in (False, True)
    ]
    for name in args.names:
        for settings, hinted in variants:
            server = Server(args.workers, os.path.join(args.models, f"ggml-{name}.bin"), vad)
            try:
                for kind, language, index, text in clips(samples):
                    path = os.path.join(args.clips, clip_name(kind, language, index))
                    with open(path, "rb") as handle:
                        wav = handle.read()
                    seconds = seconds_of(path)
                    fields = {
                        "response_format": "verbose_json",
                        "no_language_probabilities": "true",
                        "temperature": "0",
                        "language": "auto",
                    }
                    if hinted:
                        fields["prompt"] = samples["hint"]
                    if settings != "default":
                        fields.update(short_audio_fields(seconds))
                    if settings == "own-window":
                        # The first request leaves this clip's window for the second to detect with.
                        server.detect(wav, fields)
                    reply = server.detect(wav, fields)
                    detected = CODES.get(reply.get("language", ""), reply.get("language", ""))
                    rows.append(
                        {
                            "model": name,
                            "settings": settings,
                            "hinted": hinted,
                            "kind": kind,
                            "language": language,
                            "index": index,
                            "seconds": round(seconds, 2),
                            "detected": detected,
                            "text": reply.get("text", "").strip(),
                        }
                    )
            finally:
                server.stop()
            print(f"{name}: {settings} hint={hinted} done", file=sys.stderr)
    with open(args.output, "w", encoding="utf-8") as handle:
        json.dump(rows, handle, ensure_ascii=False, indent=1)
    summarize(rows)


def summarize(rows):
    def group(keys):
        table = collections.defaultdict(list)
        for row in rows:
            table[tuple(row[key] for key in keys)].append(row)
        return table

    print("\n### Detection rate\n")
    print("| Model | Request | Hint | Kind | Detected correctly | Seconds (min-max) |")
    print("|---|---|---|---|---|---|")
    for (model, settings, hinted, kind), items in sorted(group(("model", "settings", "hinted", "kind")).items()):
        right = sum(1 for row in items if row["detected"] == row["language"])
        low = min(row["seconds"] for row in items)
        high = max(row["seconds"] for row in items)
        print(f"| {model} | {settings} | {'yes' if hinted else 'no'} | {kind} | {right}/{len(items)} | {low}-{high} |")

    print("\n### Phrases by length (default request, no hint)\n")
    print("| Model | Under 1.0 s | 1.0 to 1.3 s | 1.3 s or longer |")
    print("|---|---|---|---|")
    for model in sorted({row["model"] for row in rows}):
        cells = []
        for low, high in ((0, 1.0), (1.0, 1.3), (1.3, 99)):
            clipsin = [
                row for row in rows
                if row["model"] == model and row["settings"] == "default" and not row["hinted"]
                and row["kind"] == "phrase" and low <= row["seconds"] < high
            ]
            cells.append(f"{sum(r['detected'] == r['language'] for r in clipsin)}/{len(clipsin)}")
        print(f"| {model} | " + " | ".join(cells) + " |")

    print("\n### Non-English sentences and phrases detected as English\n")
    print("Out of the non-English sentences and phrases only (the mixed clips are excluded).\n")
    print("| Model | Request | Hint | Detected as English | Out of |")
    print("|---|---|---|---|---|")
    for (model, settings, hinted), items in sorted(group(("model", "settings", "hinted")).items()):
        subset = [r for r in items if r["language"] != "en" and r["kind"] != "mixed"]
        print(f"| {model} | {settings} | {'yes' if hinted else 'no'} | {sum(r['detected'] == 'en' for r in subset)} | {len(subset)} |")

    print("\n### Phrase detection by language (default request, no hint)\n")
    languages = sorted({row["language"] for row in rows})
    print("| Model | " + " | ".join(languages) + " |")
    print("|---|" + "---|" * len(languages))
    for model in sorted({row["model"] for row in rows}):
        cells = []
        for language in languages:
            clipsin = [
                row for row in rows
                if row["model"] == model and row["settings"] == "default" and not row["hinted"]
                and row["kind"] == "phrase" and row["language"] == language
            ]
            cells.append(f"{sum(r['detected'] == r['language'] for r in clipsin)}/{len(clipsin)}")
        print(f"| {model} | " + " | ".join(cells) + " |")

    print("\n### Mixed sentences (prose in one language, identifiers in English)\n")
    for row in sorted(rows, key=lambda r: (r["model"], r["settings"], r["hinted"], r["language"])):
        if row["kind"] == "mixed" and row["model"] == "small" and row["settings"] == "default":
            print(f"- small, default, hint {'yes' if row['hinted'] else 'no'}, {row['language']}: {row['text']!r}")

    print("\n### Flips: tail language differs from the first chunk\n")
    print("| Model | Request | Hint | Recordings | Flips | First chunk right, tail wrong |")
    print("|---|---|---|---|---|---|")
    by_key = {(r["model"], r["settings"], r["hinted"], r["kind"], r["language"], r["index"]): r for r in rows}
    for (model, settings, hinted), items in sorted(group(("model", "settings", "hinted")).items()):
        pairs = flips = fixable = 0
        for row in items:
            if row["kind"] != "sentence":
                continue
            tail = by_key.get((model, settings, hinted, "phrase", row["language"], row["index"]))
            if tail is None:
                continue
            pairs += 1
            if tail["detected"] != row["detected"]:
                flips += 1
            if row["detected"] == row["language"] and tail["detected"] != tail["language"]:
                fixable += 1
        print(f"| {model} | {settings} | {'yes' if hinted else 'no'} | {pairs} | {flips} | {fixable} |")

    print("\n### Wrong detections\n")
    for (model, settings, hinted), items in sorted(group(("model", "settings", "hinted")).items()):
        wrong = [row for row in items if row["detected"] != row["language"]]
        if not wrong:
            continue
        print(f"{model}, {settings}, hint {'yes' if hinted else 'no'}:")
        for row in wrong:
            print(f"  {row['kind']} {row['language']} #{row['index']} ({row['seconds']} s) -> {row['detected']}: {row['text'][:60]!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    gen = commands.add_parser("generate")
    gen.add_argument("out")
    gen.set_defaults(run=generate)
    run = commands.add_parser("measure")
    run.add_argument("--workers", required=True, help="directory with whisper-server")
    run.add_argument("--models", required=True, help=f"directory with ggml-<name>.bin and {SILERO}")
    run.add_argument("--clips", required=True, help="directory written by generate")
    run.add_argument("--names", nargs="+", default=["tiny", "base", "small"])
    run.add_argument("--output", required=True)
    run.set_defaults(run=measure)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
