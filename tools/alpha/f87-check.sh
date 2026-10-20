#!/bin/bash
# f87: format check, then the suites of the crates changed.
set -e
cargo fmt --check
cargo test -p wavcraft-core
cargo test -p wavcraft
