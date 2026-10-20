#!/bin/bash
# f67: format, lint, the core and app suites, then the scripted run.
set -e
cd "$(dirname "$0")/../.."
cargo fmt --all
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core
cargo test -p wavcraft
bash tools/alpha/f67.sh
