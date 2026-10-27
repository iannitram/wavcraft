#!/bin/bash
# f92: format, lint and test the crates this feature changes.
cd "$(dirname "$0")/../.."
cargo fmt
cargo fmt --check || exit 1
cargo clippy --all-targets -- -D warnings || exit 1
cargo test -p wavcraft-core -p wavcraft || exit 1
