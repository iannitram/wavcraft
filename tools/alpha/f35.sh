#!/bin/bash
# f35 scripted run: build the release app, then drive Save As (WAV/AIFF/FLAC) through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f35.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
