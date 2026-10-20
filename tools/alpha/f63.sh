#!/bin/bash
# f63 (Open Recent, Import, Files-panel columns): build the release app and drive it in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f63.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
