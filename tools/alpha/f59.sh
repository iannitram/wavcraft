#!/bin/bash
set -e
# The queue's shared target dir can hold another worktree's build of the same crate, which cargo
# trusts when its sources look older: touch ours so they are rebuilt.
find crates apps -name '*.rs' -exec touch {} +
cargo build --release -p wavcraft
python3 tools/alpha/f59.py --exe "$CARGO_TARGET_DIR/release/wavcraft"
