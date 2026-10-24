#!/bin/bash
# f43 scripted run: build the release app, then drive Save Selection As and Export (queued as one command).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f43.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
