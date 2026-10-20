#!/usr/bin/env bash
# f119 alpha: build the release app and drive the clip properties through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f119.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
