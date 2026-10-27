#!/bin/bash
# Build the release app and run the Markers panel verbs' scripted check in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f39.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
