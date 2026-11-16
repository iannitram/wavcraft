#!/bin/bash
# G3.5 (Match Loudness on the long kit file, measured with ffmpeg): build the release app and drive it.
# Needs G35_WAV=<the long kit WAV>, made with `cargo run --release -p wavcraft-testkit -- <folder> --long`.
set -e
if [ -z "$G35_WAV" ]; then
  echo "G35_WAV is not set: point it at the long kit WAV" >&2
  exit 2
fi
cargo build --release -p wavcraft
python3 tools/alpha/g35.py --exe "${CARGO_TARGET_DIR:-target}/release/wavcraft" --wav "$G35_WAV"
