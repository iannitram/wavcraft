#!/bin/bash
# Format, lint and test the crates f13 changed (queued as one command).
set -e
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
