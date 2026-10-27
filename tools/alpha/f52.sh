#!/bin/bash
# f52 scripted run: build the release app, then drive the frequency axis and status bar.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f52.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
