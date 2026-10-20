#!/bin/bash
cd "$(dirname "$0")/../.."
cargo fmt --all && git diff --stat
cargo clippy --workspace --all-targets -- -D warnings || exit 1
cargo test -p wavcraft-core -p wavcraft
