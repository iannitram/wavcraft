#!/bin/bash
# f54 (latency compensation, input monitoring; C010 C025 C050): build the release app and run the scripted check, in one queued command.
set -e
export ALPHA_SCRATCH="$HOME/dev/craft/_wt/scratch"
mkdir -p "$ALPHA_SCRATCH"
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
python3 tools/alpha/f54.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
