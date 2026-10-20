#!/bin/bash
# f72 (synchronise selection, zoom and playhead across files): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f72.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
