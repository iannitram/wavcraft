#!/bin/bash
# f07: format check, clippy, both suites, then the scripted run (one queued command).
set +e
cargo fmt --check; echo "FMT $?"
cargo clippy --all-targets -- -D warnings; echo "CLIPPY $?"
cargo test -p wavcraft-core -p wavcraft 2>&1 | grep -E "test result|FAILED|panicked"; echo "TESTS done"
bash tools/alpha/f07.sh; echo "ALPHA $?"
