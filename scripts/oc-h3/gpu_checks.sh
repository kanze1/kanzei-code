#!/usr/bin/env bash
set -euo pipefail
H3_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
trap 'printf "failed\n" > "$H3_ROOT/logs/gpu-check-state"' ERR
printf 'running\n' > "$H3_ROOT/logs/gpu-check-state"
: "${CUDA_VISIBLE_DEVICES:?Set CUDA_VISIBLE_DEVICES to the selected numeric GPU indices}"
: "${H3_NUMA_NODE:?Set H3_NUMA_NODE to the selected GPUs NUMA node}"
test "$(cat "$H3_ROOT/logs/install-state")" = installed
"$H3_ROOT/bootstrap/bin/python" "$H3_ROOT/scripts/launch.py" --check-only
export CUDA_DEVICE_ORDER=PCI_BUS_ID
export OMP_NUM_THREADS=4
export MKL_NUM_THREADS=4
export TOKENIZERS_PARALLELISM=false
export NCCL_P2P_DISABLE=1
"$H3_ROOT/runtime/bin/python" -c 'import imageio_ffmpeg, pathlib, sys; p=pathlib.Path(sys.executable).parent / "ffmpeg"; p.exists() or p.symlink_to(imageio_ffmpeg.get_ffmpeg_exe())'
timeout 90 numactl --cpunodebind="$H3_NUMA_NODE" --preferred="$H3_NUMA_NODE" \
  "$H3_ROOT/runtime/bin/torchrun" --standalone --nproc-per-node=2 "$H3_ROOT/scripts/gpu_smoke.py"
timeout 90 "$H3_ROOT/runtime/bin/sglang" serve --help > "$H3_ROOT/logs/serve-help.txt" 2>&1
printf 'passed\n' > "$H3_ROOT/logs/gpu-check-state"
