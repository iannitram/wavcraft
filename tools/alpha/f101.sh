#!/usr/bin/env bash
# Alpha run for f101 (floating panels remember their monitor): build the release app, then drive it.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f101.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
