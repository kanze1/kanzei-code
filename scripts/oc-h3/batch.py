"""Run a reviewed list of character clips serially on the existing server."""
import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("plan", type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
plan = json.loads(args.plan.read_text(encoding="utf-8"))
progress = root / "logs" / (args.plan.stem + "-status.json")
completed = []
for job in plan["jobs"]:
    result_file = root / "outputs" / job["name"] / "result.json"
    if result_file.exists():
        result = json.loads(result_file.read_text())
        movie = root / "outputs" / job["name"] / "sample.mp4"
        with movie.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if digest != result["sha256"]:
            raise RuntimeError("Existing output checksum mismatch: " + job["name"])
        completed.append(job["name"])
        continue
    progress.write_text(json.dumps({"completed": completed, "current": job["name"], "total": len(plan["jobs"])}))
    command = [sys.executable, str(root / "scripts/generate.py"), "--mode", plan["mode"],
        "--profile", str(root / "scripts" / plan["profile"]), "--image", str(root / "inputs" / job["image"]),
        "--last-image", str(root / "inputs" / job["last_image"]), "--prompt-file", str(root / "scripts" / job["prompt"]),
        "--seconds", str(job["seconds"]), "--name", job["name"]]
    with (root / "logs" / ("generate-" + job["name"] + ".log")).open("w") as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    completed.append(job["name"])
    print(json.dumps({"completed": completed, "total": len(plan["jobs"])}), flush=True)
progress.write_text(json.dumps({"completed": completed, "current": None, "total": len(plan["jobs"]), "done": True}))
