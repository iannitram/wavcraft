#!/usr/bin/env bash
# f117 alpha: build the release app and drive Save/Open Session through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f117.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
