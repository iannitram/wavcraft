#!/bin/bash
set -e
cd "$(dirname "$0")/../.."
cargo fmt
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
