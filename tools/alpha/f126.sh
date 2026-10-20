#!/usr/bin/env bash
# f126 alpha: build the release app, punch in/out over the control channel with the fake input.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f126.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
