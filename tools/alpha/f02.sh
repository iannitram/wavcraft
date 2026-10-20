#!/usr/bin/env bash
# Alpha run for f02 (panel registry and Window menu): build the release app, then drive it.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f02.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
