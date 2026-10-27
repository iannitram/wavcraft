#!/bin/bash
# Format, then run the core tests (queued).
set -e
cargo fmt
cargo test -p wavcraft-core 2>&1 | tail -80
