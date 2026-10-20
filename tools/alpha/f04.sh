#!/bin/bash
# f04 (multiple open documents): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f04.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
