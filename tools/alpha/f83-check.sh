#!/bin/bash
# f83 checks: format, lints, and the suites of the crates changed.
set -e
cd "$(dirname "$0")/../.."
cargo fmt --check || { cargo fmt; echo "fmt changed files"; exit 1; }
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
