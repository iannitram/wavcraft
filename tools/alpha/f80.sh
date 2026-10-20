#!/bin/bash
# Scripted run for f80: build the release app and drive settings export/import, reset and About.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f80.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
