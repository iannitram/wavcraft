#!/usr/bin/env bash
# f120 alpha: build the release app, record takes over the control channel with the fake input, kill it mid-take.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f120.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
