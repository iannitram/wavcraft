#!/bin/bash
# f22 Mix Paste: build the release app and drive it through the control channel in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f22.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
