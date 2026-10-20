#!/bin/bash
# f45 (Files panel): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f45.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
