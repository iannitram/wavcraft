#!/bin/bash
# Build the release app and run the marker-edits/save scripted check in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f17.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
