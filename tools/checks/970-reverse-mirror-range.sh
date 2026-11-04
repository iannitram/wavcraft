#!/usr/bin/env bash
# Locked check for #970: oneshot::reverse takes its source frame from mirror_range, no inline copy
# of the mirror arithmetic is left, and Reverse behaves as before. Run from the repo root.
set -u
fail=0
ONE=crates/core/src/effects/oneshot.rs
REV=crates/core/src/effects/reverse.rs
no() { echo "FAIL: $1"; fail=1; }

# The body of `pub fn reverse` in oneshot.rs (up to the next top-level item).
body=$(awk '/^pub fn reverse\(/{on=1} on{print} on&&/^}/{exit}' "$ONE")
[ -n "$body" ] || no "oneshot::reverse not found"
echo "$body" | grep -q 'mirror_range(' || no "oneshot::reverse does not call mirror_range"
if grep -nE 'end - \(at - start\)' "$ONE"; then no "the inline mirror arithmetic is still in oneshot.rs"; fi
echo "$body" | grep -q 'offline::reverse(' || no "the one-shot must keep soundcraft-dsp's offline::reverse"
if echo "$body" | grep -q 'reverse_frames'; then no "the one-shot must not switch to reverse_frames"; fi

# reverse.rs: the doc stops claiming an inline copy; the helpers stay.
if grep -n 'computes the same mirror range inline' "$REV"; then no "reverse.rs module doc still says the one-shot computes the range inline"; fi
head -8 "$REV" | grep -q 'mirror_range' || no "reverse.rs module doc should still name mirror_range"
grep -q '^pub fn mirror_range' "$REV" || no "mirror_range is gone from reverse.rs"
grep -q '^pub fn reverse_frames' "$REV" || no "reverse_frames is gone from reverse.rs"

# Behaviour is unchanged and nothing new warns.
r=$(cargo test -p wavcraft-core --lib effects::reverse 2>&1); echo "$r" | grep -q 'test result: ok. 5 passed' || { echo "$r" | tail -15; no "lib effects::reverse: expected 5 passed"; }
r=$(cargo test -p wavcraft-core --test reverse 2>&1); echo "$r" | grep -q 'test result: ok. 7 passed' || { echo "$r" | tail -15; no "tests/reverse.rs: expected 7 passed"; }
r=$(cargo test -p wavcraft-core --test oneshot_effects reverse 2>&1); echo "$r" | grep -q 'test result: ok. 2 passed' || { echo "$r" | tail -15; no "oneshot_effects reverse: expected 2 passed"; }
r=$(cargo check --workspace 2>&1) || { echo "$r" | tail -15; no "cargo check --workspace"; }
if echo "$r" | grep -q '^warning'; then echo "$r" | grep -A6 '^warning' | head -20; no "cargo check --workspace warns"; fi

[ $fail -eq 0 ] && echo "PASS: #970 locked check"
exit $fail
