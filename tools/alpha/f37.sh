#!/bin/bash
# f37 (Amplitude Statistics): build the release app and drive it, in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f37.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
