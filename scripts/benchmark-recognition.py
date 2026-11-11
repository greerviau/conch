#!/usr/bin/env python3
"""Measures request time and word error rate of whisper-server request settings.

Each setting is sent to one warm `whisper-server` started the way Conch starts it
(voice detection on, 4 threads). The language goes in each request, as Conch
sends it. Any Whisper model file works, English-only or multilingual. The clips
are either synthetic, made with the macOS `say` command (English), or your own
recordings: put `name.wav` and `name.txt` (the words you said) side by side in a
folder and pass `--clips`.

    python3 scripts/benchmark-recognition.py \
        --model "$HOME/Library/Application Support/app.conch.desktop/ggml-small.en.bin"

Standard library only. Real speech, rooms and microphones are only measured when
you pass `--clips`. `--out` also records every clip's text, so failures can be
read.
"""

import argparse
import json
import math
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SENTENCES = [
    "Rename the helper in useAuthStore so it returns a promise instead of a callback.",
    "Add a retry with exponential backoff to the TaskRunner and cap it at five attempts.",
    "The podGC setting is unset, so completed pods stay around until the next patch is rejected.",
    "Open a pull request that moves the config loader into its own module and updates the imports.",
    "Why does the build fail when maxGroups is zero? Check the validation before the scheduler starts.",
    "Write a unit test that covers the empty list, a single item, and a list with duplicate names.",
    "Summarize the diff between main and the release branch, and list every file that changed.",
    "Stop the worker, clear the cache directory, and then start it again with verbose logging turned on.",
]
LONG_SENTENCES = [
    SENTENCES[0] + " " + SENTENCES[1] + " " + SENTENCES[2],
    SENTENCES[3] + " " + SENTENCES[4] + " " + SENTENCES[5],
]
VOICES = ["Samantha", "Daniel", "Karen", "Alex"]
CONTEXT_FRAMES_PER_SECOND = 50


def settings(margin_frames):
    """Request fields per setting name, given the clip length in seconds."""
    names = {
        "default": lambda seconds: {},
        "fallback-off": lambda seconds: {"temperature_inc": "0"},
        "no-timestamps": lambda seconds: {
            "temperature_inc": "0",
            "no_timestamps": "true",
        },
    }
    for margin in margin_frames:
        names[f"audio-ctx+{margin}"] = lambda seconds, margin=margin: {
            "temperature_inc": "0",
            "no_timestamps": "true",
            "audio_ctx": str(
                min(1500, math.ceil(seconds * CONTEXT_FRAMES_PER_SECOND) + margin)
            ),
        }
        names[f"audio-ctx-only+{margin}"] = lambda seconds, margin=margin: {
            "audio_ctx": str(
                min(1500, math.ceil(seconds * CONTEXT_FRAMES_PER_SECOND) + margin)
            ),
        }
    return names


def words(text):
    return re.findall(r"[a-z0-9']+", text.lower())


def edit_distance(a, b):
    previous = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        current = [i]
        for j, y in enumerate(b, 1):
            current.append(
                min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (x != y))
            )
        previous = current
    return previous[-1]


def synthetic_clips(directory):
    if shutil.which("say") is None or shutil.which("afconvert") is None:
        sys.exit("Synthetic clips need macOS `say` and `afconvert`; pass --clips instead.")
    clips = []
    for number, sentence in enumerate(SENTENCES + LONG_SENTENCES):
        for voice in VOICES:
            stem = directory / f"{voice}-{number}"
            aiff = stem.with_suffix(".aiff")
            subprocess.run(["say", "-v", voice, "-o", str(aiff), sentence], check=True)
            subprocess.run(
                [
                    "afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1",
                    str(aiff), str(stem.with_suffix(".wav")),
                ],
                check=True,
            )
            clips.append((stem.with_suffix(".wav"), sentence))
    return clips


def folder_clips(directory):
    clips = []
    for wav in sorted(Path(directory).glob("*.wav")):
        reference = wav.with_suffix(".txt")
        if not reference.is_file():
            sys.exit(f"{wav.name} has no {reference.name} beside it")
        clips.append((wav, reference.read_text()))
    if not clips:
        sys.exit(f"No .wav files in {directory}")
    return clips


def seconds_of(path):
    with wave.open(str(path)) as handle:
        return handle.getnframes() / handle.getframerate()


def multipart(fields, wav):
    boundary = uuid.uuid4().hex
    body = bytearray()
    for name, value in fields.items():
        body += f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
    body += (
        f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="clip.wav"\r\n'
        "Content-Type: audio/wav\r\n\r\n"
    ).encode()
    body += wav + f"\r\n--{boundary}--\r\n".encode()
    return bytes(body), f"multipart/form-data; boundary={boundary}"


def free_port():
    import socket

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Server:
    def __init__(self, executable, model, vad):
        self.port = free_port()
        self.token = uuid.uuid4().hex
        self.process = subprocess.Popen(
            [
                str(executable), "--host", "127.0.0.1", "--port", str(self.port),
                "--model", str(model), "--threads", "4",
                "--suppress-nst", "--vad", "--vad-model", str(vad),
            ],
            env={**os.environ, "CONCH_WORKER_TOKEN": self.token},
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        deadline = time.time() + 60
        while time.time() < deadline:
            try:
                self.request("health")
                return
            except OSError:
                time.sleep(0.1)
        self.close()
        sys.exit("whisper-server did not start")

    def request(self, path, data=None, content_type=None):
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/{path}", data=data,
            headers={"Authorization": f"Bearer {self.token}"},
        )
        if content_type:
            request.add_header("Content-Type", content_type)
        with urllib.request.urlopen(request, timeout=120) as response:
            return response.read()

    def recognize(self, wav, extra):
        body, content_type = multipart(
            {"response_format": "json", "temperature": "0", **extra}, wav
        )
        started = time.perf_counter()
        reply = self.request("inference", body, content_type)
        elapsed = time.perf_counter() - started
        return " ".join(json.loads(reply)["text"].split()), elapsed

    def close(self):
        self.process.terminate()
        self.process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--language", default="en", help="language code or auto, sent with each request")
    parser.add_argument("--vad", type=Path, help="default: ggml-silero-v5.1.2.bin beside the model")
    parser.add_argument("--server", type=Path, default=ROOT / "src-tauri/workers/whisper-server")
    parser.add_argument("--clips", type=Path, help="folder of name.wav and name.txt pairs")
    parser.add_argument("--margins", type=int, nargs="+", default=[25, 50, 100],
                        help="extra audio_ctx frames (20 ms each) beyond the clip length")
    parser.add_argument("--only", nargs="+", help="setting names to run, for example default audio-ctx+50")
    parser.add_argument("--repeats", type=int, default=2, help="requests per clip and setting; the fastest counts")
    parser.add_argument("--out", type=Path, help="write the results as JSON")
    args = parser.parse_args()
    vad = args.vad or args.model.parent / "ggml-silero-v5.1.2.bin"
    with tempfile.TemporaryDirectory() as scratch:
        clips = folder_clips(args.clips) if args.clips else synthetic_clips(Path(scratch))
        server = Server(args.server, args.model, vad)
        try:
            server.recognize(clips[0][0].read_bytes(), {"language": args.language})
            results = {}
            for name, fields in settings(args.margins).items():
                if args.only and name not in args.only:
                    continue
                errors = total = 0
                times = []
                details = []
                for wav, reference in clips:
                    data = wav.read_bytes()
                    seconds = seconds_of(wav)
                    # Keep the text and the time of the same request: the fastest.
                    text, best = min(
                        (
                            server.recognize(
                                data, {"language": args.language, **fields(seconds)}
                            )
                            for _ in range(args.repeats)
                        ),
                        key=lambda reply: reply[1],
                    )
                    clip_errors = edit_distance(words(reference), words(text))
                    errors += clip_errors
                    total += len(words(reference))
                    times.append(best)
                    details.append({
                        "clip": wav.name, "seconds": seconds, "reference": reference,
                        "text": text, "errors": clip_errors, "request_seconds": best,
                    })
                results[name] = {
                    "word_error_rate": errors / total,
                    "mean_seconds": statistics.mean(times),
                    "median_seconds": statistics.median(times),
                    "clips": len(clips),
                    "words": total,
                    "details": details,
                }
                print(f"{name:22} WER {100 * errors / total:5.1f}%  mean {1000 * statistics.mean(times):6.0f} ms  median {1000 * statistics.median(times):6.0f} ms", flush=True)
        finally:
            server.close()
    if args.out:
        args.out.write_text(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
