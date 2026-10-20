#!/bin/bash
set -eu
cd "$(dirname "$0")/../.."
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft
