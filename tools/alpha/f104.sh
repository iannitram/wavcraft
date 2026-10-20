#!/bin/bash
# f104 (Mixer panel): build the release app and drive the Mixer through the control channel, in one queued command.
set -e
cargo build --release -p wavcraft
python3 tools/alpha/f104.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
