#!/bin/bash
# f06 alpha run: build the release app, then drive the background job runner through the
# control channel. Queue this file: wavq.sh f06 <worktree> bash tools/alpha/f06.sh
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f06.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
