#!/bin/bash
# f90 alpha: build the release app and run the scripted Bars and Beats check (queue this file).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f90.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
