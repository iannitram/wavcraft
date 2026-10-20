#!/bin/bash
# f65: format, lint and test in one queued command.
set -e
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core -p wavcraft
