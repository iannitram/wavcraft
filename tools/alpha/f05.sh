#!/bin/bash
# f05: build the release app and drive it through the control channel (queued as one command).
set -e
cd "$(dirname "$0")/../.."
find crates apps -name "*.rs" -exec touch {} +
cargo build --release -p wavcraft
python3 tools/alpha/f05.py --exe "$CARGO_TARGET_DIR/release/wavcraft" "$@"
