"""Apply the narrow SGLang 0.5.20 H3/LoRA compatibility correction."""
import hashlib
import importlib.metadata
import json
from pathlib import Path
from types import SimpleNamespace

root = Path(__file__).resolve().parent.parent
packages = root / "runtime/lib/python3.12/site-packages"
version = next(dist.version for dist in importlib.metadata.distributions(path=[str(packages)])
               if dist.metadata["Name"].lower() == "sglang")
if version != "0.5.20":
    raise RuntimeError(f"This patch was reviewed for SGLang 0.5.20, found {version}")
target = packages / "sglang/multimodal_gen/runtime/models/dits/minimax_h3.py"
before = '''def _accepts_mxfp8_input(linear: nn.Module) -> bool:
    return linear.quant_method is not None and linear.quant_method.accepts_mxfp8_input(
        linear
    )
'''
after = '''def _accepts_mxfp8_input(linear: nn.Module) -> bool:
    # LoRA wrappers accept ordinary tensors and may omit quant_method.
    # Keep those wrappers on the existing BF16 path, including their adapter.
    quant_method = getattr(linear, "quant_method", None)
    return quant_method is not None and quant_method.accepts_mxfp8_input(linear)
'''
source = target.read_text()
backup = root / "logs/minimax_h3.before_lora_compat.py"
if source.count(before) == 1:
    if not backup.exists():
        backup.write_text(source)
    target.write_text(source.replace(before, after, 1))
elif source.count(after) != 1:
    raise RuntimeError("Unexpected H3 source; refusing an ambiguous replacement")

namespace = {}
exec("from __future__ import annotations\n" + after, namespace)
accepts = namespace["_accepts_mxfp8_input"]
assert accepts(SimpleNamespace()) is False
assert accepts(SimpleNamespace(quant_method=None)) is False
for expected in (False, True):
    method = SimpleNamespace(accepts_mxfp8_input=lambda layer, result=expected: result)
    assert accepts(SimpleNamespace(quant_method=method)) is expected
record = {"sglang": version, "patch": "h3_lora_optional_quant_method",
          "file": str(target), "backup": str(backup),
          "before_sha256": hashlib.sha256(backup.read_bytes()).hexdigest(),
          "after_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
          "capability_checks": "4 passed"}
(root / "logs/compat-patches.json").write_text(json.dumps(record, indent=2))
print(json.dumps(record))
