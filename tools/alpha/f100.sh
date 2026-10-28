#!/bin/bash
# f100 scripted run: build the release app, then save markers into an AIFF and reopen it.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f100.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
