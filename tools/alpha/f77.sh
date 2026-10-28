#!/bin/bash
# f77 (multi-input recording, C034): build the release app and run the scripted check, in one queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f77.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
