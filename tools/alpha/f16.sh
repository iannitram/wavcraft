#!/usr/bin/env bash
# f16 (tools bar, right-click extend): build the release app and drive it through the control channel.
set -euo pipefail
cargo build --release -p wavcraft
python3 tools/alpha/f16.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
