#!/bin/bash
# f31 alpha run: build the release app and drive the Effects Rack and presets.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f31.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
