#!/bin/bash
# f33 alpha: build the release app and run the scripted check (Match Loudness) in ONE queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f33.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
