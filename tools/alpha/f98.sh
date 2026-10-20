#!/bin/bash
# f98: XMP search, display sets. Build the release app, then drive it (queue: one command).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f98.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
