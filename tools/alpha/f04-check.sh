#!/bin/bash
# fmt, clippy and the two suites for f04, in one queued command.
set -e
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft
