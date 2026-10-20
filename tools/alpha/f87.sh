#!/bin/bash
# f87 scripted run: build the release app, then drive size estimates, export presets and AIFF-C through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f87.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
