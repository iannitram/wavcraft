#!/bin/bash
set -e
cargo fmt
cargo fmt --check
cargo clippy --all-targets -- -D warnings
