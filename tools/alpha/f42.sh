#!/bin/bash
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f42.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
