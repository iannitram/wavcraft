#!/bin/bash
# f84 scripted run: build the release app and drive it, in one queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f84.py --exe "$CARGO_TARGET_DIR/release/wavcraft" ${F84_ARGS:-}
