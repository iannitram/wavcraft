#!/bin/bash
# f106 alpha: build the release app and run the scripted metronome check (queue this file).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f106.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
