#!/bin/bash
# f97 scripted run: build the release app and drive per-setup audio hardware defaults.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f97.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
