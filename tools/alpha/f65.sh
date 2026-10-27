#!/bin/bash
# f65: build the release app and run the spectral-display scripted check in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f65.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
