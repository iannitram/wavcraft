#!/bin/bash
# Format, clippy, and both crates' tests, in one queued command.
set -e
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core -p wavcraft
