#!/bin/bash
# f73 scripted run: build the release app and drive Batch Process through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f73.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
