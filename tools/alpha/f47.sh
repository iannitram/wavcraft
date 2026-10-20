#!/bin/bash
# f47 alpha: build the release app and run the scripted SMPTE / snap-to-frames check (queue this file).
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f47.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
