#!/bin/bash
set -e
cd "$(dirname "$0")/../.."
cargo fmt
cargo clippy --all-targets -- -D warnings
bash tools/alpha/f94.sh
