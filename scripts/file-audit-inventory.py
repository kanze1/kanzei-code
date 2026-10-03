"""Rebuild the lexical source inventory; this does not certify review coverage."""
import json
import re
import subprocess
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
out = root / "docs/reviews/file-audit-2026-10-03"
out.mkdir(parents=True, exist_ok=True)
packages = []
workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8-sig"))
for member in workspace["workspace"]["members"]:
    data = tomllib.loads((root / member / "Cargo.toml").read_text(encoding="utf-8-sig"))
    packages.append({
        "name": data["package"]["name"],
        "manifest": member + "/Cargo.toml",
        "dependencies": [k for k in data.get("dependencies", {}) if k.startswith("kanzei")],
        "dev_dependencies": [k for k in data.get("dev-dependencies", {}) if k.startswith("kanzei")],
    })
records = []
for path in sorted((root / "crates").rglob("*")):
    if not path.is_file() or path.suffix not in (".rs", ".js", ".mjs"):
        continue
    if "vendor" in path.parts or "binaries" in path.parts:
        continue
    text = path.read_text(encoding="utf-8-sig")
    api = [{"kind": m[1], "name": m[2], "line": text.count("\n", 0, m.start()) + 1}
           for m in re.finditer(r"(?m)^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?(?:unsafe\s+)?(fn|struct|enum|trait|type|const)\s+(\w+)", text)]
    records.append({
        "file": path.relative_to(root).as_posix(),
        "lines": len(text.splitlines()),
        "modules": re.findall(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;", text),
        "imports": re.findall(r"(?m)^\s*(?:pub\s+)?use\s+([^;]+);", text),
        "public_api": api,
    })
result = {
    "baseline": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
    "method": "Lexical source inventory, including cfg/test branches. Not a resolved call graph. Review judgments live only in report.md.",
    "packages": packages,
    "files": records,
}
(out / "inventory.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"{len(records)} files, {sum(r['lines'] for r in records)} lines; {out}")
