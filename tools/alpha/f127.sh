#!/bin/bash
# f127 alpha run: build the release app and drive Stretch and Pitch and the pitch readout.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f127.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
