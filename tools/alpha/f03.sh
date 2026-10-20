#!/bin/bash
# f03 alpha run: build the release app and drive the one-shot effects through its control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f03.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
