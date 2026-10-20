#!/bin/bash
# f67 scripted run: build the release app and drive the new preference pages through PREF get/set.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f67.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
