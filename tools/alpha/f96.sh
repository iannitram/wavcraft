#!/usr/bin/env bash
# Alpha run for f96 (workspace keys): build the release app, then drive it.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f96.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
