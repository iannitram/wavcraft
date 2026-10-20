#!/bin/bash
cargo fmt --all
cargo fmt --all --check && echo FMT-OK
cargo clippy --all-targets -- -D warnings 2>&1 | tail -40 && echo CLIPPY-DONE
cargo test -p wavcraft-core -p wavcraft 2>&1 | grep -E "test result|FAILED|failed|panicked"
bash tools/alpha/f86.sh
