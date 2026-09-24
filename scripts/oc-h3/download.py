"""Download the pinned OC H3 base and adapters into the isolated deployment."""

import argparse
import fnmatch
import hashlib
import json
import time
from pathlib import Path

from huggingface_hub import HfApi, hf_hub_download, snapshot_download


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    profile = json.loads((root / "scripts/profile.json").read_text())
    status_path = root / "logs/download-status.json"
    status = {"started_at": time.time(), "stage": "metadata", "complete": False}

    def save(**updates):
        status.update(updates, updated_at=time.time())
        temporary = status_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(status, indent=2), encoding="utf-8")
        temporary.replace(status_path)
        print(json.dumps(status), flush=True)

    try:
        base = profile["base"]
        info = HfApi().model_info(base["repo"], revision=base["revision"], files_metadata=True)
        expected = [
            {"path": f.rfilename, "bytes": f.size}
            for f in info.siblings
            if any(fnmatch.fnmatch(f.rfilename, pattern) for pattern in base["allow_patterns"])
        ]
        (root / "logs/base-files.json").write_text(json.dumps(expected, indent=2))
        save(stage="base", base_expected_bytes=sum(f["bytes"] for f in expected))
        snapshot_download(
            repo_id=base["repo"], revision=base["revision"],
            allow_patterns=base["allow_patterns"], local_dir=root / "models/base",
            max_workers=4,
        )
        invalid = [f["path"] for f in expected
                   if not (root / "models/base" / f["path"]).is_file()
                   or (root / "models/base" / f["path"]).stat().st_size != f["bytes"]]
        if invalid:
            raise RuntimeError(f"Missing or incomplete base files: {invalid}")
        adapters = {}
        for name in ("larry", "lightx2v"):
            save(stage=name)
            item = profile[name]
            path = Path(hf_hub_download(
                repo_id=item["repo"], revision=item["revision"], filename=item["filename"],
                local_dir=root / "models" / name,
            ))
            with path.open("rb") as handle:
                digest = hashlib.file_digest(handle, "sha256").hexdigest()
            adapters[name] = {"file": str(path), "bytes": path.stat().st_size, "sha256": digest}
        save(stage="complete", complete=True, adapters=adapters, elapsed_seconds=time.time()-status["started_at"])
    except Exception as error:
        save(stage="failed", error=f"{type(error).__name__}: {error}")
        raise


if __name__ == "__main__":
    main()
