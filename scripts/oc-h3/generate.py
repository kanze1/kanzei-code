"""Submit one reproducible OC clip and retain the request, status, and output."""
import argparse
import hashlib
import json
import shutil
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["larry", "base", "lightx2v"], default="larry")
    parser.add_argument("--seconds", type=float, default=6.0)
    parser.add_argument("--short-edge", type=int, default=768)
    parser.add_argument("--name", default=None)
    parser.add_argument("--prompt-file", type=Path, default=None)
    parser.add_argument("--profile", type=Path, default=None)
    parser.add_argument("--image", type=Path, default=None)
    parser.add_argument("--last-image", type=Path, default=None)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--wait-ready", action="store_true")
    args = parser.parse_args()
    if not 4.0 <= args.seconds <= 15.0:
        parser.error("MiniMax-H3 duration must be between 4 and 15 seconds")
    root = Path(__file__).resolve().parent.parent
    profile_path = (args.profile or root / "scripts/profile.json").resolve()
    profile = json.loads(profile_path.read_text())
    first_image = (args.image or root / "inputs/master-soft-v5.png").resolve(strict=True)
    images = [(0, first_image)]
    if args.last_image:
        images.append((-1, args.last_image.resolve(strict=True)))
    name = args.name or f"oc-gesture-{args.mode}-{args.short_edge}p"
    if Path(name).name != name:
        raise ValueError("Output name must be a single path component")
    output = root / "outputs" / name
    output.mkdir(parents=True, exist_ok=True)
    state_file = output / "status.json"
    if state_file.exists():
        raise RuntimeError(f"An existing run is recorded at {state_file}; use a new name")
    prompt_file = args.prompt_file or root / "scripts/gesture-prompt.txt"
    request = {
        "model": str(root / "models/MiniMax-H3"),
        "prompt": prompt_file.read_text(encoding="utf-8"),
        "seconds": args.seconds,
        "task": "fl2va",
        "conditions": [{"type": "image", "uri": path.as_uri(),
                        "role": "keyframe", "frame_index": frame} for frame, path in images],
        "target": {"short_edge": args.short_edge, "aspect_ratio": "auto", "duration_seconds": args.seconds},
        "num_outputs_per_prompt": 1,
        "num_inference_steps": 50 if args.mode == "base" else 9,
        "flow_shift": 6.0 if args.mode == "lightx2v" else 12.0,
        "audio_flow_shift": 3.0,
        "quality": "lossless",
        "seed": profile["seed"],
        "output_path": str(output / "service"),
    }
    (output / "request.json").write_text(json.dumps(request, indent=2), encoding="utf-8")
    input_records = []
    for frame, path in images:
        with path.open("rb") as handle:
            digest = hashlib.file_digest(handle, "sha256").hexdigest()
        input_records.append({"frame_index": frame, "path": str(path), "sha256": digest})
    (output / "inputs.json").write_text(json.dumps({"images": input_records,
        "profile": str(profile_path)}, indent=2), encoding="utf-8")
    if args.prepare_only:
        print(output / "request.json")
        return
    api = f"http://127.0.0.1:{profile['server_port']}"
    peaks = {uuid: 0 for uuid in profile["gpu_uuids"]}

    def http_json(path, payload=None):
        body = json.dumps(payload).encode() if payload is not None else None
        req = urllib.request.Request(api + path, data=body, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise RuntimeError(f"HTTP {error.code}: {error.read().decode()}") from error

    if args.wait_ready:
        print("Waiting for the H3 API before submitting the clip", flush=True)
        deadline = time.monotonic() + 1800
        while True:
            try:
                with urllib.request.urlopen(api + "/health", timeout=5) as response:
                    if response.status == 200:
                        break
            except (urllib.error.URLError, TimeoutError):
                pass
            if time.monotonic() > deadline:
                raise TimeoutError("H3 did not become ready within 30 minutes; no job submitted")
            time.sleep(5)
    launch = json.loads((root / "logs/launch.json").read_text())
    if launch["mode"] != args.mode:
        raise RuntimeError(f"The server was launched for {launch['mode']}, not {args.mode}")
    if {gpu["uuid"] for gpu in launch["gpus"]} != set(profile["gpu_uuids"]):
        raise RuntimeError("The request profile does not match the running server GPU pair")
    started = time.time()
    job = http_json("/v1/videos", request)
    job_id = job["id"]
    last_stage = None
    while True:
        snapshot = {"job": job, "elapsed_seconds": round(time.time() - started, 3), "peak_gpu_memory_mib": peaks}
        temporary = state_file.with_suffix(".tmp")
        temporary.write_text(json.dumps(snapshot, indent=2))
        temporary.replace(state_file)
        if job.get("status") != last_stage:
            print(json.dumps(snapshot), flush=True)
            last_stage = job.get("status")
        if job.get("status") in ("failed", "cancelled", "canceled"):
            raise RuntimeError(f"Video job did not complete: {job}")
        if job.get("status") == "completed":
            break
        if time.time() - started > 7200:
            raise TimeoutError(f"Job {job_id} exceeded two hours; the saved status identifies the existing job")
        try:
            usage = subprocess.check_output([
                "nvidia-smi", "--query-gpu=uuid,memory.used", "--format=csv,noheader,nounits"
            ], text=True, timeout=10)
            for row in usage.splitlines():
                uuid, value = [part.strip() for part in row.split(",")]
                if uuid in peaks:
                    peaks[uuid] = max(peaks[uuid], int(value))
        except (ValueError, subprocess.SubprocessError):
            pass
        time.sleep(5)
        job = http_json(f"/v1/videos/{job_id}")
    movie = output / "sample.mp4"
    with urllib.request.urlopen(api + f"/v1/videos/{job_id}/content", timeout=120) as response, movie.open("wb") as handle:
        shutil.copyfileobj(response, handle)
    with movie.open("rb") as handle:
        digest = hashlib.file_digest(handle, "sha256").hexdigest()
    result = {"output": str(movie), "bytes": movie.stat().st_size, "sha256": digest,
              "elapsed_seconds": round(time.time() - started, 3), "peak_gpu_memory_mib": peaks, "job": job}
    (output / "result.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
