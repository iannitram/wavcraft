#!/usr/bin/env bash
# Locked check for #999: apps/wavcraft/src/input.rs is renamed to input_monitor.rs, contents unchanged.
# Run from the repo root: bash tools/checks/999-input-monitor-rename.sh
set -u
fail=0
bad() { echo "FAIL: $1"; fail=1; }
src=apps/wavcraft/src
[ -e $src/input_monitor.rs ] || bad "$src/input_monitor.rs does not exist"
[ ! -e $src/input.rs ] || bad "$src/input.rs still exists"
# The renamed file is the old file byte for byte (blob hash of input.rs on main).
[ "$(git hash-object $src/input_monitor.rs 2>/dev/null)" = "072651c1d45119e1ff38ba9f4f52c9db39a0c16c" ] || bad "input_monitor.rs is not the unchanged old input.rs"
grep -qx 'mod input_monitor;' $src/main.rs || bad "main.rs has no 'mod input_monitor;'"
grep -qx 'mod input;' $src/main.rs && bad "main.rs still has 'mod input;'"
grep -qx 'mod rec_input;' $src/main.rs || bad "main.rs lost 'mod rec_input;'"
grep -qx 'use crate::input_monitor::InputMonitor;' $src/app.rs || bad "app.rs does not import crate::input_monitor::InputMonitor"
grep -qF '| `apps/wavcraft/src/input_monitor.rs` (f56) | `InputMonitor`:' docs/code-map.md || bad "the code-map row does not name input_monitor.rs"
grep -qF 'apps/wavcraft/src/input.rs' docs/code-map.md && bad "docs/code-map.md still names apps/wavcraft/src/input.rs"
if grep -rnE 'crate::input([^_a-z]|$)|mod input;' apps crates --include='*.rs'; then bad "a reference to crate::input remains"; fi
# crates/core/src/input_meter.rs is not renamed.
[ -e crates/core/src/input_meter.rs ] || bad "crates/core/src/input_meter.rs is gone"
grep -qx 'pub mod input_meter;' crates/core/src/lib.rs || bad "lib.rs lost 'pub mod input_meter;'"
# Line endings are kept: these files have no CR on main.
for f in $src/main.rs $src/app.rs docs/code-map.md; do
  grep -q $'\r' "$f" && bad "$f gained CR line endings"
done
cargo check --workspace -q || bad "cargo check --workspace"
[ $fail -eq 0 ] && echo "PASS: #999 locked check"
exit $fail
