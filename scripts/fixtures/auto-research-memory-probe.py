"""Small, reproducible CUDA retrieval experiments for AUTO research acceptance.

These are synthetic mechanism checks, not evaluations of an LLM or paper reproductions.
"""
import argparse
import hashlib
import json
import time
from pathlib import Path

import torch
import torch.nn.functional as F


def callback(name, value):
    print("@@kanzei " + json.dumps({"t": "metric", "ts": time.time(), "name": name, "value": value}), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", choices=["freshness", "compression"], required=True)
    parser.add_argument("--role", choices=["baseline", "main", "ablation", "robustness"], required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--fail", action="store_true")
    args = parser.parse_args()
    if args.fail:
        raise RuntimeError("Injected experiment failure for recovery acceptance")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is required; this case must not silently fall back to CPU")
    torch.manual_seed(args.seed)
    torch.cuda.manual_seed_all(args.seed)
    device = "cuda"
    n, dim = 768, 128
    keys = F.normalize(torch.randn(n, dim, device=device), dim=-1)
    noise = 0.15 if args.role == "robustness" else 0.04
    queries = F.normalize(keys + noise * torch.randn_like(keys), dim=-1)
    targets = torch.arange(n, device=device)
    if args.case == "freshness":
        current = F.normalize(keys + 0.06 * torch.randn_like(keys), dim=-1)
        stale = F.normalize(keys + 0.005 * torch.randn_like(keys), dim=-1)
        memory = torch.cat([current, stale])
        values = torch.cat([targets, (targets + 1) % n])
        if args.role in ("main", "robustness"):
            # Timestamp metadata is an oracle in this controlled test.
            memory, values = current, targets
        scores = queries @ memory.T
        predictions = values[scores.argmax(dim=-1)]
        retained = memory.numel()
    else:
        retained_dim = {"baseline": 128, "main": 4, "ablation": 32, "robustness": 4}[args.role]
        memory = F.normalize(keys[:, :retained_dim], dim=-1)
        compressed_queries = F.normalize(queries[:, :retained_dim], dim=-1)
        scores = compressed_queries @ memory.T
        predictions = scores.argmax(dim=-1)
        retained = memory.numel()
    torch.cuda.synchronize()
    accuracy = float((predictions == targets).float().mean().cpu())
    result = {
        **vars(args), "accuracy": accuracy, "queries": n, "retained_values": retained,
        "device": torch.cuda.get_device_name(), "torch": torch.__version__,
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "predictions": predictions.cpu().tolist(), "targets": targets.cpu().tolist(),
        "scope": "synthetic CUDA retrieval; not an LLM benchmark or paper reproduction",
    }
    path = Path(f"measurements-{args.role}-{args.seed}.json")
    path.write_text(json.dumps(result, indent=2), encoding="utf-8")
    callback("accuracy", accuracy)
    callback("retained_values", retained)
    callback("queries", n)
    print(json.dumps({"artifact": str(path), "device": result["device"], "accuracy": accuracy}), flush=True)


if __name__ == "__main__":
    main()
