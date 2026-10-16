#!/usr/bin/env bash
# Fetch the published TAS (TASVideos 1087M, Aglar, FCEU 0.98.16) and convert it to tools/emu/tas/inputs.csv.
set -euo pipefail
dir="$(cd "$(dirname "$0")" && pwd)/tas"
mkdir -p "$dir"
curl -fsSL --retry 10 --retry-delay 30 --retry-all-errors -A 'Mozilla/5.0' -o "$dir/movie.zip" 'https://tasvideos.org/1087M?handler=Download'
python3 -I -c 'import sys, zipfile; z = zipfile.ZipFile(sys.argv[1]); open(sys.argv[2], "wb").write(z.read("aglar-batmanrotj.fcm"))' "$dir/movie.zip" "$dir/movie.fcm"
echo "47a3cf6bf16652b267b8a12dfe23b476fcc34fd78373805cd288a0384f51e72a  $dir/movie.fcm" | sha256sum -c -
python3 -I "$(dirname "$0")/fcm2csv.py" "$dir/movie.fcm" "$dir/inputs.csv"
