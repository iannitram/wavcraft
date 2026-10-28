#!/bin/bash
# Scripted run for f11 (Convert Sample Type): build the release app and drive it. Run through the queue:
#   ~/dev/craft/_tools/wavq.sh f11 <worktree> bash tools/alpha/f11.sh
set -euo pipefail
cargo build --release -p wavcraft
python3 tools/alpha/f11.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
