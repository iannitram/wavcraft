#!/usr/bin/env bash
# f110 alpha: build the release app and drive the session markers through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f110.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
