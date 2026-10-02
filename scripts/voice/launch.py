"""Manage exactly this voice runtime's two child processes under WSL2."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

import yaml
from cuda_runtime import configure_cuda

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--stop", action="store_true")
args = parser.parse_args()
root = args.root.resolve()
config_path = root / "service.json"
config = json.loads(config_path.read_text(encoding="utf-8-sig"))
pid_path = root / "supervisor.pid"
if pid_path.exists():
    try:
        pid = int(pid_path.read_text())
        command = Path(f"/proc/{pid}/cmdline").read_bytes()
        if str(Path(__file__).resolve()).encode() in command and str(root).encode() in command:
            if args.stop:
                os.kill(pid, signal.SIGTERM)
                print("Voice runtime stopping", flush=True)
                raise SystemExit(0)
            raise SystemExit("This voice runtime is already running")
    except (FileNotFoundError, ValueError):
        pass
if args.stop:
    print("Voice runtime is not running", flush=True)
    raise SystemExit(0)

import fcntl
runtime_lock = (root / "supervisor.lock").open("a")
try:
    fcntl.flock(runtime_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    raise SystemExit("This voice runtime is already running")

# Use the installed version's native deployment schema, only override capacity.
import importlib.util
package = Path(importlib.util.find_spec("vllm_omni").origin).parent
deployment = yaml.safe_load((package / "deploy/qwen3_tts.yaml").read_text())
for stage in deployment["stages"]:
    stage["max_num_seqs"] = 1
    stage["gpu_memory_utilization"] = .30 if stage["stage_id"] == 0 else .12
    stage["trust_remote_code"] = False
    if stage["stage_id"] == 0:
        # Single-user speech needs one short sequence, not a multi-user KV pool.
        # WSL may report Windows' occupied VRAM as free; use an explicit bound.
        stage["kv_cache_memory_bytes"] = 512 * 1024 * 1024
        stage["max_num_batched_tokens"] = 2048
        stage["max_model_len"] = 2048
        stage["default_sampling_params"]["max_tokens"] = 2048
deploy_path = root / "qwen3-tts.yaml"
deploy_path.write_text(yaml.safe_dump(deployment, sort_keys=False))
environment = dict(os.environ, HF_HUB_OFFLINE="1", HF_HUB_DISABLE_TELEMETRY="1", DO_NOT_TRACK="1", OMP_NUM_THREADS="4", VLLM_WORKER_MULTIPROC_METHOD="spawn")
configure_cuda(environment)
children = []
logs = []
closing = False

def request_stop(*_):
    global closing
    closing = True

signal.signal(signal.SIGTERM, request_stop)
signal.signal(signal.SIGINT, request_stop)
pid_path.write_text(str(os.getpid()))
try:
    for name, command in [
        ("tts", [str(Path(sys.executable).with_name("vllm")), "serve", config["tts_model_path"], "--omni", "--host", "127.0.0.1", "--port", "8091", "--served-model-name", config["tts_model"], "--deploy-config", str(deploy_path)]),
        ("gateway", [sys.executable, str(Path(__file__).with_name("service.py")), "--config", str(config_path)]),
    ]:
        log = (root / f"{name}.log").open("a", buffering=1)
        logs.append(log)
        child = subprocess.Popen(command, env=environment, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        children.append(child)
    print(f"Voice runtime started; supervisor PID {os.getpid()}", flush=True)
    while not closing:
        if any(child.poll() is not None for child in children):
            print("A voice process exited; see gateway.log and tts.log", flush=True)
            break
        time.sleep(.5)
finally:
    for child in children:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
    for child in children:
        try:
            child.wait(timeout=12)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
    if pid_path.exists() and pid_path.read_text().strip() == str(os.getpid()):
        pid_path.unlink()
    for log in logs:
        log.close()
