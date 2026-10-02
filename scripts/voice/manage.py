"""Windows launcher with argument arrays, including paths containing spaces."""
import argparse
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("action", choices=["start", "stop"])
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--distribution", default="Ubuntu")
args = parser.parse_args()
root = args.root.resolve()
if not (root / "service.json").is_file():
    raise SystemExit("Run scripts/voice/setup.ps1 first")

def linux(path):
    path = path.resolve()
    if len(path.drive) != 2:
        raise ValueError("Runtime and scripts must be on a local Windows drive")
    return "/mnt/" + path.drive[0].lower() + path.as_posix()[2:]

prefix = ["wsl.exe", "-d", args.distribution, "--exec"]
environment = dict(os.environ, WSL_UTF8="1")
home = subprocess.check_output(prefix + ["printenv", "HOME"], env=environment, creationflags=subprocess.CREATE_NO_WINDOW).decode("utf-8").strip()
command = prefix + [home + "/.venvs/kanzei-voice/bin/python", linux(Path(__file__).with_name("launch.py")), "--root", linux(root)]
if args.action == "stop":
    raise SystemExit(subprocess.call(command + ["--stop"], env=environment, creationflags=subprocess.CREATE_NO_WINDOW))
with (root / "supervisor.log").open("ab") as out, (root / "supervisor-error.log").open("ab") as error:
    process = subprocess.Popen(command, env=environment, creationflags=subprocess.CREATE_NO_WINDOW, stdin=subprocess.DEVNULL, stdout=out, stderr=error)
print(f"Voice service starting (launcher PID {process.pid}). Logs: {root}")
