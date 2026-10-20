#!/bin/bash
# f84: format, then the suites of the crates changed, then clippy (all queued as one command).
set -e
cd "$(dirname "$0")/../.."
cargo fmt --all
cargo fmt --all --check
cargo test -p wavcraft-core
cargo test -p wavcraft
cargo clippy --all-targets -- -D warnings
