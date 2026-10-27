#!/bin/bash
# f81: format, lint, test (core + app); the scripted run is f81.sh.
set -e
cargo fmt
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
