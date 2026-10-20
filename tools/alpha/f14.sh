#!/bin/bash
# f14 alpha run: build the release app and drive the effect parameter dialog and Repeat Last Command.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f14.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
