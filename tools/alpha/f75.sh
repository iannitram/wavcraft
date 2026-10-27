#!/bin/bash
# A134 scripted run: build the release app and drive a generated 5.1 file through it.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f75.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
