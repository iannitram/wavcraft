#!/bin/bash
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f64.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
