#!/bin/bash
# f32 (keyboard shortcuts as data): build the release app, then the scripted run in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f32.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
