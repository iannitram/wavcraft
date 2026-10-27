#!/usr/bin/env bash
# f109 alpha: build the release app and drive the automation lanes through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f109.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
