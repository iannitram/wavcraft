#!/bin/bash
# f92 scripted run: build the release app, then drive the frequency axis and status bar.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f92.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
