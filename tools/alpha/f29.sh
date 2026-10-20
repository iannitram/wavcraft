#!/bin/bash
# f29 alpha run: build the release app and drive Capture Noise Print and Noise Reduction.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f29.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
