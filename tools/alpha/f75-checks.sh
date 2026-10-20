#!/bin/bash
cd "$(dirname "$0")/../.."
cargo fmt --all && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
