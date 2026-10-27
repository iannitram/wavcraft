#!/bin/bash
# f129 scripted run: build the release app and drive a Video track through the control channel,
# in one queued command (the shared release binary must not change between build and run).
set -eu
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f129.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
