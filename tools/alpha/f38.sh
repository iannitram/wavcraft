#!/bin/bash
# f38: chunk copy-through + BWF metadata. Build the release app, then drive it (queue: one command).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f38.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
