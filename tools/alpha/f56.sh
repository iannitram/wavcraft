#!/bin/bash
# f56 alpha: build the release app and run the scripted check (input meter and mic check, fake input only)
# in ONE queued command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f56.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
