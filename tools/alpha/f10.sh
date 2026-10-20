#!/bin/bash
# f10 scripted run: build the release app and drive Auto Save / crash recovery (one queued command).
set -eu
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f10.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
