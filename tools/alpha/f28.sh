#!/bin/bash
# f28 alpha run: build the release app and drive the dynamics effects through the control channel.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f28.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
