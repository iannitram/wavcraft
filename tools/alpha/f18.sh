#!/bin/bash
# f18 alpha: build the release app and run the scripted time-display check (queue this file).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f18.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
