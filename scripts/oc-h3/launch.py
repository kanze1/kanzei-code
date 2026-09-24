"""Launch H3 only on the approved, idle pair in the expected NUMA node."""
import argparse
import csv
import json
import os
import shlex
import socket
import subprocess
from pathlib import Path


def query(fields, kind="gpu"):
    out = subprocess.check_output([
        "nvidia-smi", f"--query-{kind}={fields}", "--format=csv,noheader,nounits"
    ], text=True)
    return [[part.strip() for part in row] for row in csv.reader(out.splitlines())]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["base", "larry", "lightx2v"], default="larry")
    parser.add_argument("--check-only", action="store_true")
    parser.add_argument("--profile", type=Path, default=None)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    profile_path = (args.profile or root / "scripts/profile.json").resolve()
    profile = json.loads(profile_path.read_text())
    expected = profile["gpu_uuids"]
    selected = []
    inventory = query("index,uuid,pci.bus_id,memory.used")
    # SGLang 0.5.20's NVML helper requires numeric CUDA_VISIBLE_DEVICES.
    # Resolve the approved UUIDs freshly, and verify CUDA's PCI ordering agrees.
    pci_order = sorted(inventory, key=lambda row: row[2].lower())
    if any(int(row[0]) != ordinal for ordinal, row in enumerate(pci_order)):
        raise RuntimeError("CUDA PCI order differs from NVML indices; numeric masking needs review")
    for index, uuid, pci, memory_used in inventory:
        if uuid not in expected:
            continue
        domain, bus, device = pci.lower().split(":")
        pci_path = f"{int(domain, 16):04x}:{bus}:{device}"
        node = int((Path("/sys/bus/pci/devices") / pci_path / "numa_node").read_text())
        if node != profile["numa_node"]:
            raise RuntimeError(f"GPU {index} belongs to NUMA {node}, expected {profile['numa_node']}")
        if float(memory_used) > 128:
            raise RuntimeError(f"GPU {index} has {memory_used} MiB in use")
        selected.append({"physical_index": int(index), "uuid": uuid, "numa": node})
    if {gpu["uuid"] for gpu in selected} != set(expected):
        raise RuntimeError("The two approved GPUs were not both found")
    occupied = [row for row in query("gpu_uuid,pid", "compute-apps") if row[0] in expected]
    if occupied:
        raise RuntimeError(f"Approved GPUs already have compute processes: {occupied}")
    with socket.socket() as sock:
        if sock.connect_ex(("127.0.0.1", profile["server_port"])) == 0:
            raise RuntimeError("The H3 port is already occupied")
    # SGLang recognizes native H3 by the local directory's final component.
    # Keep downloaded files untouched and expose a correctly named alias.
    model_path = root / "models/MiniMax-H3"
    if not args.check_only:
        if not model_path.exists():
            model_path.symlink_to("base", target_is_directory=True)
        if model_path.resolve() != (root / "models/base").resolve():
            raise RuntimeError("The MiniMax-H3 alias does not point to the pinned base")
    command = [
        "numactl", f"--cpunodebind={profile['numa_node']}", f"--preferred={profile['numa_node']}",
        str(root / "runtime/bin/sglang"), "serve", "--model-path", str(model_path),
        "--model-variant", "fl2va", "--num-gpus", "2", "--tp-size", "2",
        "--ulysses-degree", "1", "--encoder-parallel", "auto",
        "--performance-mode", "memory", "--layerwise-offload-components", "dit,text_encoder,vae",
        "--dit-offload-prefetch-size", "1", "--dit-layerwise-resident-layers", str(profile["dit_resident_layers"]),
        "--enable-torch-compile", "false", "--host", "127.0.0.1", "--port", str(profile["server_port"]),
        "--output-path", str(root / "outputs/service"), "--input-save-path", str(root / "inputs/uploads"),
        "--warmup-mode", "off",
    ]
    if args.mode != "base":
        adapter = profile[args.mode]
        command += ["--lora-path", str(root / "models" / args.mode), "--lora-weight-name", adapter["filename"],
                    "--lora-nickname", f"h3-{args.mode}", "--lora-scale", "1.0", "--lora-merge-mode", "auto"]
    visible_devices = ",".join(str(next(gpu["physical_index"] for gpu in selected if gpu["uuid"] == uuid))
                               for uuid in expected)
    event = {"mode": args.mode, "profile": str(profile_path), "gpus": selected,
             "cuda_visible_devices": visible_devices, "command": command}
    print(json.dumps(event, indent=2), flush=True)
    print(shlex.join(command), flush=True)
    if args.check_only:
        return
    if not json.loads((root / "logs/download-status.json").read_text()).get("complete"):
        raise RuntimeError("Pinned model download is incomplete")
    environment = dict(os.environ)
    environment.update(CUDA_VISIBLE_DEVICES=visible_devices, CUDA_DEVICE_ORDER="PCI_BUS_ID",
                       OMP_NUM_THREADS="8", MKL_NUM_THREADS="8", TOKENIZERS_PARALLELISM="false",
                       SGLANG_USE_RUNAI_MODEL_STREAMER="0")
    cuda_home = root / "runtime/lib/python3.12/site-packages/nvidia/cu13"
    if (cuda_home / "bin/nvcc").is_file():
        # CUDA wheels use lib/ and a versioned runtime name, whereas JIT
        # extension builds expect the conventional toolkit linker layout.
        if not (cuda_home / "lib64").exists():
            (cuda_home / "lib64").symlink_to("lib", target_is_directory=True)
        if not (cuda_home / "lib/libcudart.so").exists():
            (cuda_home / "lib/libcudart.so").symlink_to("libcudart.so.13")
        environment["CUDA_HOME"] = str(cuda_home)
    environment["PATH"] = str(root / "runtime/bin") + os.pathsep + environment.get("PATH", "")
    headers = root / "tools/python-dev/usr/include"
    if (headers / "python3.12/Python.h").is_file():
        include_paths = [str(headers / "python3.12"), str(headers)]
        if environment.get("CPATH"):
            include_paths.append(environment["CPATH"])
        environment["CPATH"] = os.pathsep.join(include_paths)
    if profile.get("nccl_p2p_disable"):
        environment["NCCL_P2P_DISABLE"] = "1"
    (root / "logs/launch.json").write_text(json.dumps(event, indent=2))
    # exec keeps this PID, so stop.py must not reuse a previous deployment PID.
    (root / "logs/serve.pid").write_text(str(os.getpid()) + "\n")
    os.execvpe(command[0], command, environment)


if __name__ == "__main__":
    main()
