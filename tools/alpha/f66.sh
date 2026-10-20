#!/bin/bash
# f66 alpha: build the release app and run the scripted time-entry check (queue this file).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f66.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
