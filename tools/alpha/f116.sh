#!/usr/bin/env bash
# f116 alpha: build the release app and drive the clip-source round trip through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f116.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
