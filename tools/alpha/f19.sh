#!/bin/bash
# f19 scripted run: build the release app and drive the zoom navigator, buttons and ruler.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f19.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
