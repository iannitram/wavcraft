#!/bin/bash
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f99.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
