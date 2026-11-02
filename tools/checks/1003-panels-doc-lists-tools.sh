#!/usr/bin/env bash
# Locked check for #1003: the PANELS doc line in control.rs lists the Tools panel, and the panels
# unit tests still pass with none deleted or ignored.
# Run from the repo root: bash tools/checks/1003-panels-doc-lists-tools.sh
set -u
fail=0
bad() { echo "FAIL: $1"; fail=1; }
line=$(grep -F '//! - `PANELS`' apps/wavcraft/src/control.rs | head -1)
[ -n "$line" ] || bad "the PANELS doc line is not in control.rs"
# The real PANELS reply lists every REGISTRY panel in order: editor first, then
# waveform ... transport, time, zoom, tools, files, match.
case "$line" in
  *'OK editor=<0|1> waveform=<0|1> '*) ;;
  *) bad "the PANELS doc line does not start with 'OK editor=<0|1> waveform=<0|1>'" ;;
esac
case "$line" in
  *'transport=<0|1> time=<0|1> zoom=<0|1> tools=<0|1> files=<0|1> match=<0|1> '*) ;;
  *) bad "the PANELS doc line does not list 'transport time zoom tools files match' in REGISTRY order" ;;
esac
# REGISTRY still has the tools panel (docked left), and no panels test is deleted or ignored.
grep -qF 'PanelInfo { id: "tools", title: "Tools", default_dock: DockSide::Left }' crates/core/src/panels.rs || bad "REGISTRY lost the tools panel"
[ "$(grep -c '#\[test\]' crates/core/src/panels.rs)" -ge 31 ] || bad "a test in panels.rs was deleted (31 on main)"
grep -q '#\[ignore' crates/core/src/panels.rs && bad "a test in panels.rs is #[ignore]d"
out=$(cargo test -p wavcraft-core --lib panels 2>&1)
echo "$out" | grep -qE '^test result: ok\. ' || { echo "$out" | grep -E "FAILED|panicked|error" | head -20; bad "cargo test -p wavcraft-core --lib panels"; }
[ $fail -eq 0 ] && echo "PASS: #1003 locked check"
exit $fail
