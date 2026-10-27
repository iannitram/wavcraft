#!/bin/bash
# f69 scripted run: build the release app and drive Zoom to Time slots, zoom in at In/Out, and the zoom factor.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f69.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
