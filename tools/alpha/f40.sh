#!/bin/bash
# Scripted run for f40 (dither and noise shaping): build the release app and drive it. Run through the queue:
#   ~/dev/craft/_tools/wavq.sh f40 <worktree> bash tools/alpha/f40.sh
set -euo pipefail
cargo build --release -p wavcraft
python3 tools/alpha/f40.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
