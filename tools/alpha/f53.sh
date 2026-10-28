#!/bin/bash
# f53 scripted run: build the release app, then drive the Transport marker / start / end buttons.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f53.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
