#!/bin/bash
# f107 scripted run: build the release app, then the golden task (open, edit, save, reopen).
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/golden.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
python3 tools/alpha/f102.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
