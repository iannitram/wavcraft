#!/usr/bin/env bash
# f118: format, lint and the suites of the crates changed.
set -e
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
