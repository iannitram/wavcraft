#!/usr/bin/env bash
# f123 alpha: build the release app and drive the audio types through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f123.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
