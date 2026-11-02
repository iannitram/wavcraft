#!/usr/bin/env bash
# Locked check for #993 (the app half): scrub_drag asks for the unit with the document's tempo.
# Run from the repo root: bash tools/checks/993-scrub-uses-tempo.sh
set -u
fail=0
f=apps/wavcraft/src/app.rs
if grep -n 'time_format\.unit_at(' "$f"; then echo "FAIL: app.rs still calls unit_at (no tempo)"; fail=1; fi
if ! grep -q 'time_format\.unit_at_with(.*&self\.tempo' "$f"; then echo "FAIL: scrub_drag does not call unit_at_with(..., &self.tempo)"; fail=1; fi
[ $fail -eq 0 ] && echo "PASS: #993 app check"
exit $fail
