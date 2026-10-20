#!/bin/bash
cargo fmt --all --check && echo FMT-OK
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -v "Compiling\|Checking" | tail -30
cargo test -p wavcraft-core -p wavcraft -p wavcraft-testkit 2>&1 | grep -E "^(test result|error|failures|---- |test .*FAILED)|panicked|-->" | head -60
