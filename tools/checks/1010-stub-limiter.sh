#!/usr/bin/env bash
# Locked check for #1010: the stub limiter is gone and nothing else moved.
set -u
fail=0
if git grep -n -e "true_peak_limiter" -e "TruePeakLimiter" -- crates apps; then echo "FAIL: stub limiter still referenced"; fail=1; fi
if [ -e crates/core/src/dsp/true_peak_limiter.rs ]; then echo "FAIL: true_peak_limiter.rs still exists"; fail=1; fi
# The expected value is the rustfmt-normalised hash of the originally locked blob d22d119 (identical to today's file after formatting),
# so a format-only pass (like 25dba55) does not trip the check, but any content change does.
norm_dir=$(mktemp -d)
trap 'rm -rf "$norm_dir"' EXIT
cp crates/core/src/match_loudness.rs rustfmt.toml "$norm_dir"/
if ! rustfmt --edition 2024 "$norm_dir/match_loudness.rs"; then echo "FAIL: rustfmt match_loudness.rs"; fail=1; fi
if [ "$(git hash-object "$norm_dir/match_loudness.rs")" != "cb8361947bd89e5ab9650b7cb1df03c3af3a2b56" ]; then echo "FAIL: match_loudness.rs changed"; fail=1; fi
cargo check --workspace -q || { echo "FAIL: cargo check --workspace"; fail=1; }
out=$(cargo test -p wavcraft-core --lib 2>&1)
if echo "$out" | grep -q "true_peak_limiter::tests"; then echo "FAIL: stub limiter tests still run"; fail=1; fi
if echo "$out" | grep -qE "^test result: FAILED"; then echo "$out" | grep -E "FAILED|panicked" | head -20; echo "FAIL: wavcraft-core lib tests"; fail=1; fi
[ $fail -eq 0 ] && echo "PASS: #1010 locked check"
exit $fail
