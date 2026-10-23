#!/bin/bash
# f88: channel names (iXML) + RIFF INFO tags. Build the release app, then drive it (queue: one command).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f88.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
