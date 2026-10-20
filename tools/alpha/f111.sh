#!/usr/bin/env bash
# f111 alpha: build the release app and drive Mixdown through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f111.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
