#!/bin/bash
# Build the release app and run the Insert > Audio from File scripted check (queued as one command).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f13.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
