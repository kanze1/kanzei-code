#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
destination="$root/tools/python-dev"
mkdir -p "$destination"
cd "$destination"
apt-get download libpython3.12-dev
for archive in ./libpython3.12-dev_*.deb; do
  dpkg-deb -x "$archive" "$destination"
  sha256sum "$archive" > "$root/logs/python-headers.sha256"
done
test -f "$destination/usr/include/python3.12/Python.h"
printf 'Python development headers are ready at %s\n' "$destination"
