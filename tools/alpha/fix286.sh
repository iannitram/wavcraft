#!/bin/bash
# fix286 scripted run: build the release app, then check that cancelling the export dialog changes no saved option.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/fix286.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
