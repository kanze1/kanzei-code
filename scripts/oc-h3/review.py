"""Validate a rendered clip and extract review frames without changing it."""
import argparse
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("movie", type=Path)
    parser.add_argument("--output", type=Path, help="Separate evidence directory when reviewing several movies")
    args = parser.parse_args()
    movie = args.movie.resolve(strict=True)
    output = args.output.resolve() if args.output else movie.parent / "review"
    output.mkdir(exist_ok=True, parents=True)
    metadata = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-count_frames", "-show_format", "-show_streams",
        "-of", "json", str(movie),
    ], text=True))
    (output / "media.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    # Decode every frame: metadata alone does not establish file integrity.
    subprocess.run(["ffmpeg", "-v", "error", "-xerror", "-i", str(movie),
                    "-f", "null", "-"], check=True)
    video = next(stream for stream in metadata["streams"] if stream["codec_type"] == "video")
    frame_count = int(video["nb_read_frames"])
    numerator, denominator = map(int, video["avg_frame_rate"].split("/"))
    fps = numerator / denominator
    # Cover the entire movie, including both endpoints, regardless of length.
    indices = sorted({round(index * (frame_count - 1) / 11) for index in range(12)})
    selection = "+".join(f"eq(n\\,{index})" for index in indices)
    subprocess.run([
        "ffmpeg", "-v", "error", "-y", "-i", str(movie),
        "-vf", f"select={selection},scale=320:-1,tile=4x3:padding=4:margin=4:color=0x222222",
        "-frames:v", "1", str(output / "contact-sheet.png"),
    ], check=True)
    duration = float(metadata["format"]["duration"])
    last_frame_time = (frame_count - 1) / fps
    timestamps = [round(last_frame_time * fraction, 3) for fraction in (0, .25, .5, .75, 1)]
    (output / "timeline.json").write_text(json.dumps({
        "contact_sheet_frame_indices": indices,
        "contact_sheet_seconds": [round(index / fps, 4) for index in indices],
        "full_size_frame_seconds": timestamps,
    }, indent=2), encoding="utf-8")
    for timestamp in timestamps:
        subprocess.run([
            "ffmpeg", "-v", "error", "-y", "-ss", str(timestamp), "-i", str(movie),
            "-frames:v", "1", str(output / f"frame-{timestamp:05.2f}.png"),
        ], check=True)
    result = {"decode": "passed", "duration": duration, "width": video["width"],
                      "height": video["height"], "fps": video["avg_frame_rate"],
                      "frames": frame_count, "review": str(output)}
    (output / "checks.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
