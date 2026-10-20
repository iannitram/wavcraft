#!/bin/bash
# f27 alpha run: build the release app and drive the EQ, filters, notch and DC removal.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f27.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
