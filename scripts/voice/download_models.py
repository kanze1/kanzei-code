"""Download pinned inference assets; leave the supplied voice project unchanged."""
import argparse
import json
from pathlib import Path
from huggingface_hub import HfApi, snapshot_download

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, required=True)
args = parser.parse_args()
assets = [
    ("tts", "Qwen/Qwen3-TTS-12Hz-1.7B-Base", "fd4b254389122332181a7c3db7f27e918eec64e3", ["*.json", "*.txt", "*.safetensors"]),
    ("asr", "Systran/faster-whisper-small", None, ["config.json", "model.bin", "tokenizer.json", "vocabulary.*", "preprocessor_config.json"]),
]
records = []
for name, repo, revision, patterns in assets:
    revision = revision or HfApi().model_info(repo).sha
    target = args.root / "models" / name
    print(f"Downloading {repo} at {revision}", flush=True)
    snapshot_download(repo, revision=revision, local_dir=target, allow_patterns=patterns, max_workers=4)
    records.append({"name": name, "repo": repo, "revision": revision, "path": str(target)})
args.root.mkdir(parents=True, exist_ok=True)
(args.root / "models.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
print("Voice models ready", flush=True)
