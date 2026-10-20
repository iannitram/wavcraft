#!/bin/bash
# f20 scripted run: build the release app and drive the preferences through the control channel.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f20.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
