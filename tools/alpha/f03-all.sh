#!/bin/bash
# f03: every check in one queue slot; keeps going after a failure so one run shows everything.
cd "$(dirname "$0")/../.."
cargo fmt --check 2>&1 | tail -40; echo "== fmt rc=${PIPESTATUS[0]}"
cargo test -p wavcraft-core 2>&1 | grep -E "^(error|warning)|test result|Running|FAILED|panicked|failed|-->" | head -80; echo "== core rc=${PIPESTATUS[0]}"
cargo test -p wavcraft 2>&1 | grep -E "^(error|warning)|test result|FAILED|panicked|-->" | head -40; echo "== app rc=${PIPESTATUS[0]}"
cargo clippy --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)|-->" | head -40; echo "== clippy rc=${PIPESTATUS[0]}"
bash tools/alpha/f03.sh 2>&1 | tail -40; echo "== alpha rc=${PIPESTATUS[0]}"
