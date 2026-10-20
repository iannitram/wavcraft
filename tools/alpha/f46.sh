#!/bin/bash
# f46 (shortcut editor): build the release app, then the scripted run in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f46.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
