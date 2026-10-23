#!/bin/bash
set -e
# The queue's shared target dir can hold another worktree's build of the same crate, which cargo
# trusts when its sources look older: touch ours so they are rebuilt.
find crates apps -name '*.rs' -exec touch {} +
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p wavcraft-core -p wavcraft
