#!/bin/bash
set -euo pipefail
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core -p wavcraft
