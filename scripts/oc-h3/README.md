# OC H3 deployment

## Current v6 package (2026-09-24)

The active reference is `master-soft-v5.png`; the runtime package is
`crates/kanzei-app/ui/assets/oc/character-v6.json`. GPU 4 and 5 in NUMA 1
completed the v6 actions and the service has been stopped. Recheck availability
with `profile-soft-v5.json` before any future launch.

`action-plan-v6.json` and `batch.py` retain the nine generation jobs. Raw idle
candidates are reduced with `stabilize_idle.py`; `analyze_motion.py` produces
per-frame mouth tracking. `pack.py` accepts only reviewed clips. The separately
generated reply hold remains a candidate: truncating it midway caused a pose
mismatch, so the active graph joins the approved v5 entrance directly to its
matching exit, with the held pose already contained in those videos.

Local commands after generation and visual review:

```powershell
python scripts/oc-h3/pack.py
node scripts/oc-clip-director-smoke.mjs
node scripts/ui-oc-companion-smoke.mjs
node scripts/oc-preview.mjs --export --v6
```

The preview exports deterministic frames through `window.ocStudio.exportFrames`.
Use `oc-export-chunk.cjs` in a Playwright CLI session with `ocExportMode` set to
`demo` or `idle`. `review.py <movie> --output <directory>` keeps independent
decode and frame-review evidence for the two final movies. See
`docs/design/oc-production.md` for the active graph and validation scope.

## Historical deployment setup

The isolated server directory is `/home/p2522808/kanzei-h3`, reached through SSH alias `gpu`. `profile.json` pins the model repositories and GPUs 1 and 2 in NUMA 0. The v5 revision uses `profile-soft-v5.json`: GPUs 4 and 5 in NUMA 1, because GPUs 1 and 2 were occupied when rechecked. Each launch validates the selected pair again; neither profile assumes that an earlier idle snapshot remains valid.

Current work uses SGLang 0.5.20, BF16/FP32 base weights, Larry v4-600 EMA, TP2, and CPU layerwise offload. The source revision in the design note is a documentation reference; the installed runtime is the published 0.5.20 wheel and its resolved dependencies are recorded in `logs/requirements.lock.txt` after installation.

## Files

- `download.py`: resumable downloads of the pinned FL2VA base and two comparison adapters; saves status and verifies expected file sizes.
- `install.sh`: installs into the deployment's own `runtime` virtual environment; writes install status and a dependency lock.
- `install_media.sh`: installs FFmpeg and FFprobe 7.0.2 into this deployment only; records the archive checksum.
- `install_headers.sh`: extracts Ubuntu's Python 3.12 development headers into the deployment without installing a system package; needed by Triton's JIT compiler.
- `patch_runtime.py`: corrects H3's MXFP8 capability check for BF16 LoRA wrappers in SGLang 0.5.20. It preserves a source backup, checks the exact patch target, records hashes, and verifies four capability cases.
- `launch.py`: checks both UUIDs, actual NUMA affinity, GPU memory, compute processes, and the local port before launching. `--check-only` performs the checks without loading the model.
- `stop.py`: stops only the recorded H3 process group after checking its owner, group identity and command. `serve.pid` must have been recorded when launching with `setsid`.
- `gpu_smoke.py`: tests BF16 matrix multiplication and a two-rank NCCL all-reduce. Requires the exact approved UUID mask.
- `attention_smoke.py`: checks PyTorch SDPA and SGLang FA4 on the selected Ada GPU. Both passed with the installed runtime.
- `generate.py`: submits one OC clip through the local video API and retains request, input image hashes, job ID, sampled GPU memory peaks, timing, and MP4 checksum. `--image` and `--last-image` select first/last keyframes; `--seconds` accepts H3's 4–15-second range. `--profile` must match the running server's GPU pair. A recorded run is not silently resubmitted.
- `gesture-prompt.txt`: whole-character I2V prompt based on the approved reference and the official H3 prompt format.
- `gesture-reserved-prompt.txt`: historical performance variant, rejected for visual quality despite successful inference and decode.
- `master-soft-v5.prompt.txt` and `gesture-soft-v5-keyframe.prompt.txt`: exact built-in imagegen prompts for the lighting/material revision and its compact hand pose.
- `gesture-soft-v5-enter.txt` and `gesture-soft-v5-exit.txt`: first/last-frame-guided raising and lowering motion prompts.
- `review.py`: decodes the complete MP4, saves probe metadata, and extracts a contact sheet and full-resolution frames for visual review.

## Server commands

Copy these scripts to the deployment's `scripts/` directory and the approved `master-clean-v4.png` to `inputs/`. The image SHA256 is `9ac38f0e3b4d9e9888ee3839ce63c81000832f9a07e7d349c2c04fe91352470d`.

After installation and downloads complete:

```bash
cd /home/p2522808/kanzei-h3
bash scripts/install_media.sh
bash scripts/install_headers.sh
bootstrap/bin/python scripts/patch_runtime.py
bootstrap/bin/python scripts/launch.py --check-only
CUDA_VISIBLE_DEVICES=GPU-eff8864c-9830-23bb-4dc9-104389c8ddad,GPU-74f54117-182b-6fcc-429b-876cc116be10 \
  NCCL_P2P_DISABLE=1 OMP_NUM_THREADS=4 timeout 90 numactl --cpunodebind=0 --preferred=0 \
  runtime/bin/torchrun --standalone --nproc-per-node=2 scripts/gpu_smoke.py
bootstrap/bin/python scripts/launch.py --mode larry
```

In a separate SSH session, once the local API is ready:

```bash
cd /home/p2522808/kanzei-h3
bootstrap/bin/python scripts/generate.py --mode larry --wait-ready
```

The second historical sample used:

```bash
bootstrap/bin/python scripts/generate.py --mode larry --name oc-gesture-reserved-768p \
  --prompt-file scripts/gesture-reserved-prompt.txt
```

Both historical samples are 768 × 1152, 24 fps, 158 frames (6.583333 seconds after H3 duration alignment), generated in about four minutes each. Complete decode and local/remote SHA256 checks passed, but the user rejected the outer glow, floating sleeves and unnatural hand pose. The v5 revision uses edited references `master-soft-v5.png` and `gesture-soft-v5.png` with explicit first and last frames. Neither successful inference nor fixed endpoints alone establish acceptable motion. Evidence is recorded in `docs/design/oc-h3-deployment.md`.

The service binds to `127.0.0.1:30010`. Optional local access uses `ssh -N -L 30010:127.0.0.1:30010 gpu`.

The v5 enter segment uses the same pinned model with revised first and last frames:

```bash
bootstrap/bin/python scripts/launch.py --mode larry --profile scripts/profile-soft-v5.json
# In the generation session after launch:
bootstrap/bin/python scripts/generate.py --mode larry --profile scripts/profile-soft-v5.json \
  --image inputs/master-soft-v5.png --last-image inputs/gesture-soft-v5.png \
  --prompt-file scripts/gesture-soft-v5-enter.txt --seconds 4 --name oc-gesture-soft-v5-enter
```

For the exit segment, swap the two image arguments and use `gesture-soft-v5-exit.txt` with a new output name. Both completed as 107-frame, 4.458333-second clips. The local combined silent preview is `output/oc-h3/oc-gesture-soft-v5/sample.mp4`: 214 frames, 24 fps, 8.916667 seconds. It contains both forward-generated segments, with no reverse playback, interpolation or crossfade. Input hashes, full prompts, API requests, timings and review frames are retained. GPU 4 and 5 were released after this run.

On this server NCCL's default P2P path timed out; the same all-reduce and BF16 checks passed with `NCCL_P2P_DISABLE=1`. The launcher applies this setting. NVML's P2P capability report alone was insufficient. SGLang identifies native H3 using the directory name, so the launcher creates `models/MiniMax-H3` as a symlink to the pinned `models/base` files.

SGLang 0.5.20's NVML helper only accepts numeric GPU masks. The launcher resolves the approved UUIDs at every launch, verifies NUMA and PCI ordering, then supplies physical indices `1,2`. It also points `CUDA_HOME` to the installed `nvidia/cu13` wheel directory and supplies the conventional `lib64` and unversioned `libcudart.so` symlinks for JIT extension linking.

The SGLang H3 API counts sigma grid points: nine requested points produce eight denoiser evaluations. The `lossless` request setting disables additional caching approximations; it does not reverse the Turbo adapter. The selected model still needs visual assessment on this OC. Generated fixed speech is separate from the existing arbitrary realtime TTS and mouth-layer work.
