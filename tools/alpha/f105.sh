#!/bin/bash
# Build the release app and run the Playlist scripted check in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f105.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
