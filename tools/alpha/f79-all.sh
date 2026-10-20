#!/bin/bash
# fmt + clippy + core tests + app tests + the scripted run, one queued command.
set -e
cargo fmt
cargo fmt --check && echo FMT-OK
cargo clippy --all-targets -- -D warnings > "$CARGO_TARGET_DIR/clippy.txt" 2>&1 && echo CLIPPY-OK || { tail -60 "$CARGO_TARGET_DIR/clippy.txt"; exit 1; }
cargo test -p wavcraft-core 2>&1 | grep -E "^test result|FAILED|panicked" | awk '{p+=$4; f+=$6; print} END {print "CORE TOTAL passed", p, "failed", f}'
cargo test -p wavcraft 2>&1 | grep -E "^test result|FAILED|panicked"
bash tools/alpha/f79.sh
