#!/bin/bash
# f102 scripted run: build the release app and drive SCSOURCE test, in one queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f102.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
