#!/bin/bash
# f62: format, lint and test (queue: one command).
set -e
cd "$(dirname "$0")/../.."
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
