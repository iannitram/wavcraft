#!/bin/bash
# f01 alpha run: build the release app and drive the effect seam through the control channel.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f01.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
