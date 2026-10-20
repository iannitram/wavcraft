#!/bin/bash
# f25 (recording, C020-C024): build the release app and run the scripted check, in one queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f25.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
