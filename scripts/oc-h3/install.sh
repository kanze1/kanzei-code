#!/usr/bin/env bash
set -euo pipefail
H3_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
H3_UV=${H3_UV:-$(command -v uv || true)}
if [[ -z "$H3_UV" || ! -x "$H3_UV" ]]; then
  printf 'Set H3_UV to the uv executable path.\n' >&2
  exit 1
fi
trap 'printf "failed\n" > "$H3_ROOT/logs/install-state"' ERR
printf 'installing\n' > "$H3_ROOT/logs/install-state"
export UV_HTTP_TIMEOUT=60
export UV_HTTP_RETRIES=3
"$H3_UV" pip install --python "$H3_ROOT/runtime/bin/python" \
  'sglang[diffusion]==0.5.20' 'huggingface_hub==1.32.0' 'safetensors==0.8.0' \
  --prerelease=if-necessary --default-index https://pypi.tuna.tsinghua.edu.cn/simple
"$H3_UV" pip freeze --python "$H3_ROOT/runtime/bin/python" > "$H3_ROOT/logs/requirements.lock.txt"
"$H3_ROOT/bootstrap/bin/python" "$H3_ROOT/scripts/patch_runtime.py"
printf 'installed\n' > "$H3_ROOT/logs/install-state"
