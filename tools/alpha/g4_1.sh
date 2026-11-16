#!/bin/bash
# G4.1 (STATS and LOUDNESS against ffmpeg): build the release app and compare on the kit, in one queued command.
#   bash tools/alpha/g4_1.sh <kit folder> [--long]
set -e
cargo build --release -p wavcraft
python3 tools/alpha/g4_1.py --exe "${CARGO_TARGET_DIR:-target}/release/wavcraft" --kit "$1" "${@:2}"
