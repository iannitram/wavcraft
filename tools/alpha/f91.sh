#!/bin/bash
# f91 scripted run: build the release app, then drive raw import and W64/CAF/AU save.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f91.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
