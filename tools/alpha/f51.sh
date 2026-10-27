#!/bin/bash
# f51: save options (A262, A263). Build the release app and drive it, in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f51.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
