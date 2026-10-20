#!/bin/bash
# f34 (recording 2, C028 C029 C031 C035): build the release app and run the scripted check, in one queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f34.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
