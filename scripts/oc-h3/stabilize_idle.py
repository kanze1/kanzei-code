"""Reduce measured vertical drift in an idle VIDEO, preserving its full frames.

This postproduction pass never assembles body-part layers. Eye blinks, cloth
changes and hair motion remain from the source video. Keep the original source.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import cv2
import numpy as np


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    analysis = json.loads((args.source.parent / "review/motion-analysis.json").read_text(encoding="utf-8"))
    capture = cv2.VideoCapture(str(args.source))
    width, height = int(capture.get(cv2.CAP_PROP_FRAME_WIDTH)), int(capture.get(cv2.CAP_PROP_FRAME_HEIGHT))
    fps = capture.get(cv2.CAP_PROP_FPS)
    rows = np.array([.15, .375, .575, .885]) * height
    regions = ["head", "collar", "lower_ribs", "hips"]
    retained = np.array([.08, .20, .45, .05])
    offsets = np.stack([np.array(analysis["regions"][name]["displacement_px"])[:, 1] for name in regions], axis=1)
    # Suppress tracker noise. Boundary frames keep their original registration.
    offsets = cv2.GaussianBlur(offsets, (1, 11), 0, sigmaY=2)
    offsets -= np.linspace(offsets[0], offsets[-1], len(offsets))
    offsets *= 1 - retained
    y = np.arange(height, dtype=np.float32)
    low = np.clip(np.searchsorted(rows, y) - 1, 0, len(rows) - 2)
    ratio = np.clip((y - rows[low]) / (rows[low + 1] - rows[low]), 0, 1)
    ratio = ratio * ratio * (3 - 2 * ratio)
    grid_x, grid_y = np.meshgrid(np.arange(width, dtype=np.float32), y)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists():
        raise FileExistsError(args.output)
    command = ["ffmpeg", "-v", "error", "-f", "rawvideo", "-pixel_format", "bgr24", "-video_size", f"{width}x{height}",
        "-framerate", str(fps), "-i", "pipe:0", "-an", "-c:v", "libx264", "-preset", "slow", "-crf", "14",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(args.output)]
    encoder = subprocess.Popen(command, stdin=subprocess.PIPE)
    count = 0
    try:
        while True:
            ok, frame = capture.read()
            if not ok:
                break
            shifts = offsets[count, low] * (1 - ratio) + offsets[count, low + 1] * ratio
            stabilized = cv2.remap(frame, grid_x, (grid_y + shifts[:, None]).astype(np.float32),
                cv2.INTER_CUBIC, borderMode=cv2.BORDER_REPLICATE)
            encoder.stdin.write(stabilized.tobytes())
            count += 1
    finally:
        capture.release()
        encoder.stdin.close()
    if encoder.wait() or count != len(offsets):
        raise RuntimeError("Idle video stabilization failed")
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    result = {"operation": "whole-frame vertical drift stabilization", "source": str(args.source),
        "source_sha256": digest(args.source), "output_sha256": digest(args.output), "frames": count,
        "retained_displacement": dict(zip(regions, retained.tolist())), "row_positions": (rows / height).tolist(),
        "note": "Only authored idle clips use this pass. Gesture anatomy and limbs are never composited separately."}
    (args.output.parent / "postproduction.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
