#!/bin/bash
# f30 (Close All, Save All, Copy/Paste to New, New Audio File): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f30.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
