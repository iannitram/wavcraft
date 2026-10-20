#!/bin/bash
# f12 (A152/A153): build the release app and drive the clipboards through the control channel,
# in one queued command (the shared target dir is overwritten by whoever builds last).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f12.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
