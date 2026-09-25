#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version=7.0.2
destination="$root/tools/ffmpeg-$version"
archive="$root/tools/ffmpeg-$version-amd64-static.tar.xz"
mkdir -p "$destination"
curl --fail --location --retry 2 --max-time 180 \
  "https://johnvansickle.com/ffmpeg/releases/ffmpeg-$version-amd64-static.tar.xz" \
  --output "$archive"
sha256sum "$archive" > "$root/logs/ffmpeg-archive.sha256"
tar -xJf "$archive" --strip-components=1 -C "$destination" \
  "ffmpeg-$version-amd64-static/ffmpeg" "ffmpeg-$version-amd64-static/ffprobe" \
  "ffmpeg-$version-amd64-static/GPLv3.txt" "ffmpeg-$version-amd64-static/readme.txt"
ln -sfn "$destination/ffmpeg" "$root/runtime/bin/ffmpeg"
ln -sfn "$destination/ffprobe" "$root/runtime/bin/ffprobe"
"$root/runtime/bin/ffmpeg" -version | head -n 1
"$root/runtime/bin/ffprobe" -version | head -n 1
