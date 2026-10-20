#!/bin/bash
# f61 scripted run: build the release app and drive the audio host, buffer size and channel maps.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f61.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
