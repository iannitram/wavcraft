#!/usr/bin/env bash
# f115 alpha: build the release app and drive the track formats through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f115.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
