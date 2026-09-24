"""Package reviewed full-character videos, tracking and an authored action graph."""
import hashlib
import json
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "crates/kanzei-app/ui/assets/oc"
SOURCES = {
    "idle-breath": ("oc-idle-soft-v8", "轻呼吸"),
    "idle-observe": ("oc-idle-observe-soft-v8", "短暂移开视线"),
    "idle-quiet": ("oc-idle-quiet-soft-v8", "安静停留"),
    "listening": ("oc-listening-v6", "注视与回应"),
    "thinking": ("oc-thinking-v6", "低头思考"),
    "executing": ("oc-executing-v6", "查看与核对"),
    "blocked": ("oc-blocked-v6", "停顿与疑惑"),
    "aside": ("oc-aside-v6", "侧眼吐槽"),
    "warm": ("oc-warm-v6", "会意"),
    "complete": ("oc-complete-v6", "轻点头确认"),
    "reply-enter": ("oc-gesture-soft-v5-enter", "抬手解释"),
    "reply-exit": ("oc-gesture-soft-v5-exit", "放下手"),
}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    # Validate every source before writing any production file.
    records = {}
    for name, (job, label) in SOURCES.items():
        source = ROOT / "output/oc-h3" / job
        review = read(source / "review/visual-review.json")
        checks = read(source / "review/checks.json")
        tracking = read(source / "review/tracking.json")
        if review.get("status") != "accepted" or checks["decode"] != "passed":
            raise RuntimeError("Unreviewed video: " + name)
        if [checks["width"], checks["height"]] != [768, 1152] or checks["fps"] != "24/1":
            raise RuntimeError("Unexpected video format: " + name)
        if tracking["frames"] != checks["frames"] or len(tracking["mouth"]) != checks["frames"]:
            raise RuntimeError("Tracking duration mismatch: " + name)
        records[name] = (source, checks, tracking, label)
    destination = ASSETS / "clips-v6"
    destination.mkdir(exist_ok=True)
    clips = {}
    for name, (source, checks, tracking, label) in records.items():
        movie = destination / (name + ".mp4")
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(source / "sample.mp4"), "-an", "-c:v", "copy",
            "-movflags", "+faststart", str(movie)], check=True)
        track = destination / (name + ".json")
        shutil.copy2(source / "review/tracking.json", track)
        clips[name] = {"file": "clips-v6/" + movie.name, "tracking": "clips-v6/" + track.name,
            "sha256": sha(movie), "trackingSha256": sha(track), "frames": checks["frames"],
            "start": 0, "end": checks["frames"] / 24, "label": label,
            "source": {"job": source.name, "sha256": sha(source / "sample.mp4"),
                "postproduction": read(source / "postproduction.json") if (source / "postproduction.json").exists() else None}}
    # The approved entrance already contains a held pose; its final reference
    # matches the exit's first frame. Do not insert a separately moving torso
    # or truncate a new hold midway before joining these two authored clips.
    clips["reply-enter"].update(start=1, next="reply-exit", exit="reply-exit", exitAfter=107/24,
        protected=[[1.6, 107/24]])
    clips["reply-exit"].update(end=3, next="reply-rest", protected=[[0, 3]])
    # Neutral exit windows were selected from the reviewed full-body poses.
    # Keep a pending state change until a nod, breath or glance has settled;
    # blending halfway through it creates two visible arms or faces.
    for name, bounds in {
        "idle-breath": [.55, 6.0], "idle-observe": [.55, 7.3], "idle-quiet": [.55, 6.2],
        "listening": [.35, 3.5], "thinking": [.55, 5.5], "executing": [.55, 5.5],
        "blocked": [.2, 3.5], "aside": [.6, 4.3], "warm": [.55, 3.0], "complete": [.2, 3.0],
    }.items():
        clips[name]["protected"] = [bounds]
    clips["reply-rest"] = {**clips["idle-quiet"], "start": 7.5, "protected": [], "label": "平视回应"}
    pack = {
        "format": "kanzei.character-pack.v3", "version": "2026.09.24.6", "fps": 24,
        "size": [768, 1152], "poster": "master-soft-v5.png", "posterSha256": sha(ASSETS / "master-soft-v5.png"),
        "mouth": {"texture": "mouth-soft-v6.png", "reference": [.498, .234], "tracking": "per-frame affine"},
        "background": [218, 209, 203], "transitionMs": 180,
        "aliases": {"interrupted": "idle", "stopping": "idle", "error": "blocked"},
        "states": {
            "idle": {"clips": ["idle-breath", "idle-observe", "idle-quiet"]},
            "listening": {"clips": ["listening", "idle-breath", "idle-observe"]},
            "thinking": {"clips": ["thinking", "idle-quiet"]},
            "replying": {"clips": ["reply-enter"]},
            "executing": {"clips": ["executing", "idle-breath"]},
            "blocked": {"clips": ["blocked", "idle-quiet"]},
            "aside": {"clips": ["aside", "idle-quiet"]},
            "warm": {"clips": ["warm"], "once": True},
            "complete": {"clips": ["complete"], "once": True},
        }, "clips": clips, "demo": read(ROOT / "scripts/oc-h3/demo-v6.json"),
    }
    (ASSETS / "character-v6.json").write_text(json.dumps(pack, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"manifest": str(ASSETS / "character-v6.json"), "videos": len(records),
        "video_bytes": sum((destination/(name+".mp4")).stat().st_size for name in records)}))


if __name__ == "__main__":
    main()
