#!/usr/bin/env bash
# f114 alpha: build the release app and play a two-track session silently through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f114.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
