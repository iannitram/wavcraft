#!/bin/bash
# f23 scripted run: build the release app, then drive the transport through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f23.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
