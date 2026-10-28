#!/usr/bin/env bash
# Locked check for #971: crates/core/tests/stft.rs (a byte copy of the STFT tests in noise.rs) is
# gone, the originals stay, and nothing else changed. Run from the repo root.
set -u
fail=0
no() { echo "FAIL: $1"; fail=1; }
A=analysis_then_resynthesis_with_no_change_is_the_input_to_minus_100_db
B=the_grid_origin_and_a_chunked_pass_do_not_change_the_result

[ -e crates/core/tests/stft.rs ] && no "crates/core/tests/stft.rs still exists"
for t in $A $B; do
  n=$(git grep -c "fn $t" -- crates | awk -F: '{s+=$2} END {print s+0}')
  [ "$n" = 1 ] || no "fn $t appears $n times under crates/, expected exactly 1"
  git grep -q "fn $t" -- crates/core/tests/noise.rs || no "fn $t is not in crates/core/tests/noise.rs"
done
# noise.rs is untouched (the working tree equals HEAD) and the manifests are not edited.
git diff --quiet HEAD -- crates/core/tests/noise.rs crates/core/Cargo.toml Cargo.toml Cargo.lock || no "noise.rs, Cargo.toml or Cargo.lock changed"
# Only that one file may differ from HEAD.
other=$(git diff --name-only HEAD | grep -v '^crates/core/tests/stft.rs$' | grep -v '^tools/checks/971-')
[ -z "$other" ] || { echo "$other"; no "files other than stft.rs changed"; }

r=$(cargo test -p wavcraft-core --test noise 2>&1); echo "$r" | grep -q 'test result: ok. 8 passed; 0 failed' || { echo "$r" | tail -15; no "tests/noise.rs: expected 8 passed"; }
r=$(cargo test -p wavcraft-core --test fft_filter 2>&1); echo "$r" | grep -q 'test result: ok. 3 passed' || { echo "$r" | tail -15; no "tests/fft_filter.rs: expected 3 passed"; }
r=$(cargo test -p wavcraft-core --test stft 2>&1); echo "$r" | grep -q 'no test target named `stft`' || no "cargo should report no test target named stft"
cargo check --workspace -q 2>&1 | grep -q . && no "cargo check --workspace prints output (error or warning)"

[ $fail -eq 0 ] && echo "PASS: #971 locked check"
exit $fail
