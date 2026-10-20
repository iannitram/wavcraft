#!/bin/bash
# f80 gates, in one queued command: notices regenerate + licence gate + its unit tests, fmt, clippy, tests.
set -e
python3 tools/notices.py --write
python3 tools/notices.py --check
python3 -W ignore -m unittest discover -s tools -p 'test_*.py'
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p wavcraft-core bundle
cargo test -p wavcraft-core
cargo test -p wavcraft
