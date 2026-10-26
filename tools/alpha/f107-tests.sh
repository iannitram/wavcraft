#!/bin/bash
set -eo pipefail
cd "$(dirname "$0")/../.."
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core 2>&1 | grep -E "^test result|Running|FAILED|failed|panicked|error" 
cargo test -p wavcraft 2>&1 | grep -E "^test result|Running|FAILED|failed|panicked|error"
