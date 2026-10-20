#!/bin/bash
# fix286: format check, clippy, then the suites of the crates changed, then the scripted run.
set -e
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
bash tools/alpha/fix286.sh
bash tools/alpha/f87.sh
