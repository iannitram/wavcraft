#!/bin/bash
# f57 (Open many, Open Append to current / to new): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f57.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
