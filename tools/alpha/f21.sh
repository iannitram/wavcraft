#!/bin/bash
# f21 alpha run: build the release app and drive Generate Tones / Generate Noise.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f21.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
