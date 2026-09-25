"""Write private runtime paths and a bounded single-user Qwen deployment."""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--voice-directory", type=Path, required=True)
args = parser.parse_args()

def linux(path: Path):
    path = path.resolve()
    if not path.drive or len(path.drive) != 2:
        raise ValueError("Use a local Windows drive for the runtime")
    return "/mnt/" + path.drive[0].lower() + path.as_posix()[2:]

reference = args.voice_directory / "artifacts/trilingual-01/ja_a75.wav"
profile = json.loads((args.voice_directory / "voice-profile.json").read_text(encoding="utf-8-sig"))
if profile.get("selected_voice") != "a75" or not reference.is_file():
    raise ValueError("The selected A75 voice reference is missing")
args.root.mkdir(parents=True, exist_ok=True)
shutil.copyfile(reference, args.root / "reference-a75.wav")
config = {"asr_model": linux(args.root / "models/asr"), "tts_model_path": linux(args.root / "models/tts"),
          "tts_model": "kanzei-a75", "tts_url": "http://127.0.0.1:8091",
          "reference_audio": linux(args.root / "reference-a75.wav")}
(args.root / "service.json").write_text(json.dumps(config, indent=2), encoding="utf-8")
# Keep the installed runtime usable after the source checkout is moved or removed.
for name in ["manage.py", "launch.py", "service.py", "cuda_runtime.py"]:
    shutil.copyfile(Path(__file__).with_name(name), args.root / name)
voice_home = Path(os.environ.get("KANZEI_HOME", str(Path.home() / ".kanzei")))
voice_home.mkdir(parents=True, exist_ok=True)
launcher = {"port": 7388, "program": sys.executable,
            "args": [str(args.root.resolve() / "manage.py"), "start", "--root", str(args.root.resolve())],
            "workingDirectory": str(args.root.resolve())}
(voice_home / "voice-launcher.json").write_text(json.dumps(launcher, indent=2), encoding="utf-8")
print(f"Voice runtime configured: {args.root}")
