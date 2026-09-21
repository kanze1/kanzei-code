"""Run real-model probes and retain timing/audio evidence in an explicit output directory."""
import argparse
import json
from pathlib import Path
import time
import wave
import requests

parser = argparse.ArgumentParser()
parser.add_argument("--input", type=Path)
parser.add_argument("--text")
parser.add_argument("--language", default="auto")
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
session = requests.Session()
session.trust_env = False
report = {}
try:
    response = session.get("http://127.0.0.1:7388/health", timeout=6)
    response.raise_for_status()
    report["health"] = response.json()
    if args.input:
        started = time.monotonic()
        response = session.post("http://127.0.0.1:7388/transcribe", params={"language": args.language},
            data=args.input.read_bytes(), headers={"Content-Type": "audio/wav"}, timeout=90)
        response.raise_for_status()
        report["recognition"] = {**response.json(), "elapsed_seconds": time.monotonic() - started}
    if args.text:
        started = time.monotonic()
        received = []
        pcm = bytearray()
        with session.post("http://127.0.0.1:7388/speak", json={"text": args.text, "language": args.language}, stream=True, timeout=90) as response:
            response.raise_for_status()
            for chunk in response.iter_content(chunk_size=None):
                if chunk:
                    received.append({"at": time.monotonic() - started, "bytes": len(chunk)})
                    pcm.extend(chunk)
        assert len(pcm) > 4800 and len(pcm) % 2 == 0, "No valid speech returned"
        with wave.open(str(args.output / "speech.wav"), "wb") as output:
            output.setnchannels(1); output.setsampwidth(2); output.setframerate(24000); output.writeframes(pcm)
        report["synthesis"] = {"text": args.text, "audio_seconds": len(pcm) / 48000, "first_chunk_seconds": received[0]["at"],
            "elapsed_seconds": time.monotonic() - started, "chunks": received}
except Exception as error:
    report["error"] = f"{type(error).__name__}: {error}"
(args.output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(report, ensure_ascii=True, indent=2))
if "error" in report:
    raise SystemExit(1)
