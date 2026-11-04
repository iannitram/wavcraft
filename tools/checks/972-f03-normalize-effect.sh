#!/usr/bin/env bash
# Locked check for #972: f03.py drives Normalize through the registry effect (`EFFECT normalize ...`)
# and the control doc stops calling normalize.db the number Effects > Normalize uses. Run from the
# repo root. (f03.py needs a built app, so this checks the script and the doc as text.)
set -u
fail=0
no() { echo "FAIL: $1"; fail=1; }
F=tools/alpha/f03.py
C=apps/wavcraft/src/control.rs

python3 -m py_compile "$F" || no "f03.py does not compile"
# Normalize step: the EFFECT line is sent, the PARAM normalize.db line is not.
grep -qF 'EFFECT normalize unit=dB db=-2' "$F" || no "f03.py never sends 'EFFECT normalize unit=dB db=-2'"
grep -qF 'PARAM normalize.db' "$F" && no "f03.py still sends or checks PARAM normalize.db"
grep -qF 'check("PARAM normalize.db accepted"' "$F" && no "the 'PARAM normalize.db accepted' check is still there"
# The checks that stay: names, Amplify's PARAM, the bad-name check, the peak check, the undo.
grep -qF 'result("effect.normalize", (0, 0), "norm", "Normalize"' "$F" || no "the Normalize step must still call result(\"effect.normalize\", (0, 0), \"norm\", \"Normalize\", ...)"
grep -qF 'check("Normalize -2 dBFS: peak reached", abs(peak - 10 ** (-2 / 20)) < 1e-3' "$F" || no "the -2 dBFS peak check changed"
grep -qF 'PARAM amplify.db -6' "$F" || no "the Amplify step's PARAM amplify.db -6 is gone"
grep -qF 'PARAM volume 3' "$F" || no "the bad-name PARAM check is gone"
grep -qF 'ACTION {name}' "$F" || no "the other effects must still run through ACTION {name}"
grep -qF 'f"{name}: answered OK and changed the file"' "$F" || no "result() lost its 'answered OK and changed the file' check"
grep -qF 'f"{name}: undo step is named {want_undo!r}"' "$F" || no "result() lost its undo-label check"
grep -qF 'f"{name}: SAVE works"' "$F" || no "result() lost its SAVE check"
# The docstring says Normalize runs through EFFECT.
doc=$(awk 'NR>2 && /^"""$/{exit} NR>=2{print}' "$F")
echo "$doc" | grep -qF 'EFFECT normalize' || no "the f03.py docstring does not say Normalize runs through 'EFFECT normalize ...'"

# control.rs: the PARAM line says where normalize.db still applies and where Normalize really is.
line=$(grep -n 'PARAM <amplify.db|normalize.db>' "$C" | head -1)
[ -n "$line" ] || no "control.rs lost its PARAM <amplify.db|normalize.db> line"
echo "$line" | grep -qF 'the number Effects › Amplify or Normalize uses' && no "control.rs still says normalize.db is what Effects › Normalize uses"
echo "$line" | grep -qF 'ActionId::Normalize' || no "the control.rs PARAM line does not name the internal one-shot ActionId::Normalize"
echo "$line" | grep -qF 'EFFECT normalize unit=dB db=<n>' || no "the control.rs PARAM line does not point to 'EFFECT normalize unit=dB db=<n>'"
# Only a doc comment changes in Rust: control.rs, one `//!` line replaced, no Cargo edits.
[ "$(git diff --name-only HEAD -- '*.rs')" = "$C" ] || no "the only Rust file that may change is $C"
[ "$(git diff -U0 HEAD -- "$C" | grep -c '^-[^-]')" = 1 ] || no "control.rs: exactly one line may be removed"
git diff -U0 HEAD -- "$C" | grep '^+[^+]' | grep -qv '^+//!' && no "control.rs: added lines must be '//!' doc comments"
[ -z "$(git diff --name-only HEAD | grep -E 'Cargo|\.toml')" ] || no "Cargo files changed"
cargo check -p wavcraft -q 2>&1 | grep -q . && no "cargo check -p wavcraft prints output (error or warning)"

[ $fail -eq 0 ] && echo "PASS: #972 locked check"
exit $fail
