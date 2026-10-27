#!/bin/bash
# f68 scripted run: build the release app and drive channel layout, names and enable buttons.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f68.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
