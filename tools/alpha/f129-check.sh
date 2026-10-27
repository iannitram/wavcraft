#!/bin/bash
# f129 checks: format, clippy, then the suites of the crates changed.
set -eu
cd "$(dirname "$0")/../.."
cargo fmt
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
