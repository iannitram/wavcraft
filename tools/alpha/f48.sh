#!/bin/bash
# Build the release app and run the f48 scripted run on it (one queued command).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f48.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
