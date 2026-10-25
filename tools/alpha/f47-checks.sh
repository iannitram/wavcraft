#!/bin/bash
# f47: fmt, clippy and the tests of the crates changed (queue this file).
set -e
cargo fmt --check || { cargo fmt --all; echo "FMT FIXED (rerun)"; exit 1; }
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
