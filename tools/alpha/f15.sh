#!/usr/bin/env bash
# f15 scripted run: build the release app, then drive the History panel through the control channel.
set -euo pipefail
cargo build --release -p wavcraft
python3 tools/alpha/f15.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
