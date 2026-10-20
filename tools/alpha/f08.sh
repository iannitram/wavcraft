#!/bin/bash
# f08 alpha: selection gestures. Build the release app and drive it, in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f08.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
