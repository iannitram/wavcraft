#!/bin/bash
# f95 (Files panel channel expander): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f95.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
