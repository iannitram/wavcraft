#!/bin/bash
# f36 scripted run: build the release app, then drive scrub, pre/post-roll, rewind / fast forward and JKL shuttle.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f36.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
