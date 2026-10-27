#!/bin/bash
# f94 scripted run: build the release app and drive the Appearance colours and presets.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f94.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
