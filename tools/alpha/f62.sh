#!/bin/bash
# f62: XMP in the _PMX chunk. Build the release app, then drive it (queue: one command).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f62.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
