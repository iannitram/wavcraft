#!/bin/bash
# f81 alpha: build the release app, then drive pointer readout, status fields and tooltips' data.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f81.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
