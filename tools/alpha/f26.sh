#!/bin/bash
# f26 alpha: build the release app and run the scripted check (meters and loudness) in ONE queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f26.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
