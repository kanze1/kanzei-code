"""Bounded NCCL and CUDA check; run only with the two approved GPU UUIDs visible."""
import datetime
import json
import os
from pathlib import Path

import torch
import torch.distributed as dist

rank = int(os.environ["LOCAL_RANK"])
root = Path(__file__).resolve().parent.parent
profile = json.loads((root / "scripts/profile.json").read_text())
visible = os.environ.get("CUDA_VISIBLE_DEVICES", "").split(",")
if visible != profile["gpu_uuids"] or torch.cuda.device_count() != 2:
    raise RuntimeError("The test requires exactly the two GPU UUIDs from profile.json")
torch.cuda.set_device(rank)
dist.init_process_group("nccl", timeout=datetime.timedelta(seconds=45))
try:
    value = torch.tensor([rank + 1.0], device=f"cuda:{rank}")
    dist.all_reduce(value)
    if value.item() != 3.0:
        raise RuntimeError("NCCL all-reduce returned an unexpected value")
    matrix = torch.ones((256, 256), dtype=torch.bfloat16, device=f"cuda:{rank}")
    product = matrix @ matrix
    if not torch.all(product == 256).item():
        raise RuntimeError("BF16 matrix multiplication returned an unexpected value")
    torch.cuda.synchronize()
    props = torch.cuda.get_device_properties(rank)
    report = {
        "rank": rank, "uuid": visible[rank], "name": props.name,
        "capability": list(torch.cuda.get_device_capability(rank)),
        "torch": torch.__version__, "cuda": torch.version.cuda,
        "nccl_all_reduce": "passed", "bf16_matmul": "passed",
        "nccl_p2p_disabled": os.environ.get("NCCL_P2P_DISABLE", "0"),
    }
    (root / f"logs/gpu-smoke-{rank}.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report), flush=True)
finally:
    dist.destroy_process_group()
