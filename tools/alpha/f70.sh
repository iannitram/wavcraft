#!/bin/bash
# f70 scripted run: build the release app and drive the Auto Save / Media and Disk Cache preferences (one queued command).
set -eu
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
WAVCRAFT_SCRATCH="${WAVCRAFT_SCRATCH:-$HOME/dev/craft/_wt/scratch}" python3 tools/alpha/f70.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
