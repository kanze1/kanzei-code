"""Probe the installed exact attention paths before loading the full model."""
import json
from pathlib import Path

import torch
import torch.nn.functional as F

root = Path(__file__).resolve().parent.parent
torch.cuda.set_device(0)
q = torch.randn(64, 56, 128, device="cuda", dtype=torch.bfloat16)
k, v = torch.randn_like(q), torch.randn_like(q)
reference = F.scaled_dot_product_attention(
    q.transpose(0, 1).unsqueeze(0), k.transpose(0, 1).unsqueeze(0), v.transpose(0, 1).unsqueeze(0)
)
torch.cuda.synchronize()
report = {"torch_sdpa": "passed" if torch.isfinite(reference).all().item() else "failed"}
try:
    from sglang.kernels.ops.attention.flash_attention import flash_attn_varlen_func
    cu = torch.tensor([0, 64], device="cuda", dtype=torch.int32)
    output = flash_attn_varlen_func(q, k, v, cu_seqlens_q=cu, cu_seqlens_k=cu,
                                   max_seqlen_q=64, max_seqlen_k=64, ver=4)
    if isinstance(output, tuple):
        output = output[0]
    torch.cuda.synchronize()
    report["fa"] = "passed" if torch.isfinite(output).all().item() else "failed"
except Exception as error:
    report["fa"] = "unavailable"
    report["fa_error"] = f"{type(error).__name__}: {error}"
(root / "logs/attention-smoke.json").write_text(json.dumps(report, indent=2))
print(json.dumps(report), flush=True)
