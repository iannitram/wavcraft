#!/usr/bin/env bash
# f121 alpha: build the release app and drive the CD layout through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f121.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
