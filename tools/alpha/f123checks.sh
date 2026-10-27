#!/usr/bin/env bash
# f123: fmt, clippy and the tests of the crates changed.
set -e
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
