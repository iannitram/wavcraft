#!/bin/bash
# f45: the alpha gates in one queued command.
set -e
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
