#!/bin/bash
# f82 scripted run: build the release app, then drive menus, toolbar and tools through the control channel.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f82.py --exe "$CARGO_TARGET_DIR/release/wavcraft" --port "${F82_PORT:-47182}"
