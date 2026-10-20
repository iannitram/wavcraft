#!/bin/bash
# f31: format check, both suites, then the scripted run.
cd "$(dirname "$0")/../.."
cargo fmt --check && echo FMT-OK
cargo test -p wavcraft-core -p wavcraft 2>&1 | grep -E "^test result|^running|FAILED|panicked|failed" 
bash tools/alpha/f31.sh
