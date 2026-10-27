#!/bin/bash
# f71 alpha: build the release app and run the scripted custom and CD time formats check (queue this file).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f71.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
