#!/bin/bash
# f76: build the release app and run the scripted check (queue this whole file with wavq.sh).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f76.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
