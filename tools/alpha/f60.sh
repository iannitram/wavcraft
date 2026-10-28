#!/bin/bash
# f60 (shortcut sets, visual keyboard logic): build the release app, then the scripted run in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f60.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
