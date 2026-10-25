#!/usr/bin/env bash
# f103 alpha: build the release app and drive the multitrack basics through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f103.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
