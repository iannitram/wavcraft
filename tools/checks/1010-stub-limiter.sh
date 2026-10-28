#!/usr/bin/env bash
# Locked check for #1010: the stub limiter is gone and nothing else moved.
set -u
fail=0
if git grep -n -e "true_peak_limiter" -e "TruePeakLimiter" -- crates apps; then echo "FAIL: stub limiter still referenced"; fail=1; fi
if [ -e crates/core/src/dsp/true_peak_limiter.rs ]; then echo "FAIL: true_peak_limiter.rs still exists"; fail=1; fi
if [ "$(git hash-object crates/core/src/match_loudness.rs)" != "d22d119ea8bbbc63de892669b7d3595ee198af31" ]; then echo "FAIL: match_loudness.rs changed"; fail=1; fi
cargo check --workspace -q || { echo "FAIL: cargo check --workspace"; fail=1; }
out=$(cargo test -p wavcraft-core --lib 2>&1)
if echo "$out" | grep -q "true_peak_limiter::tests"; then echo "FAIL: stub limiter tests still run"; fail=1; fi
if echo "$out" | grep -qE "^test result: FAILED"; then echo "$out" | grep -E "FAILED|panicked" | head -20; echo "FAIL: wavcraft-core lib tests"; fail=1; fi
[ $fail -eq 0 ] && echo "PASS: #1010 locked check"
exit $fail
