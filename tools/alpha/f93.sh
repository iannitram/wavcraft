#!/bin/bash
# f93 scripted run: build the release app, then drive Save As (Ogg Vorbis) through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f93.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
