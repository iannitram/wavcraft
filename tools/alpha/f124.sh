#!/bin/bash
# f124 alpha run: build the release app and drive Reverb and Delay through the control channel.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f124.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
