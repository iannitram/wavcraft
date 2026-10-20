#!/bin/bash
# f40 checks: format, lints, both crates' tests. Run through the queue.
set -euo pipefail
cargo fmt --all
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
