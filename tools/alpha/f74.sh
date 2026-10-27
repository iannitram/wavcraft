#!/bin/bash
# f74 (A205): build the release app and run the playback-resampling scripted check.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f74.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
