#!/usr/bin/env bash
# Alpha run for f85 (workspaces): build the release app, then drive it.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f85.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
python3 tools/alpha/f41.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
python3 tools/alpha/f24.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
