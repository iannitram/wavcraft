#!/usr/bin/env bash
# f118 alpha: build the release app and drive the multitrack basics through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f118.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
