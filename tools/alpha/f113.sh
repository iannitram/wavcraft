#!/bin/bash
# f113 (Mixer on the multitrack session): build the release app and drive it through the control channel, in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f113.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
