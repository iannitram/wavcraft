#!/bin/bash
# f125 alpha run: build the release app and drive Pitch Shifter and Saturator through EFFECT.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f125.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
