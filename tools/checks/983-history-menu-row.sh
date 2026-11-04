#!/usr/bin/env bash
# Locked check for #983: the History menu button sits in a one-line row, and f78.py reads PANELS by token.
# Run from the repo root: bash tools/checks/983-history-menu-row.sh
set -u
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY' || exit 1
import re, sys
app = open("apps/wavcraft/src/app.rs").read()
f78 = open("tools/alpha/f78.py").read()
bad = []

i = app.find("fn history_panel")
j = app.find("\n    fn ", i + 10)
fn = app[i:j]
call = 'panel_menu_button(ui, maximized, "history")'
k = fn.find(call)
if k < 0:
    bad.append("history_panel no longer calls " + call)
else:
    start = fn.rfind("\n", 0, k) + 1
    before = fn[:start]
    line = fn[start:fn.find("\n", k)]
    h = fn.rfind("ui.horizontal(", 0, k)
    if "allocate_ui_with_layout(" in line:
        pass
    elif h < 0 or "});" in fn[h:k]:
        bad.append("the ≡ button must sit in `ui.horizontal(..)` or `allocate_ui_with_layout(.., interact_size.y tall ..)`, not a bare `ui.with_layout(..)` (it takes the whole panel height)")
    if "right_to_left" not in fn[start - 160:k + 160]:
        bad.append("the ≡ button must stay right-aligned (Layout::right_to_left)")

if 'a.endswith("history=0")' in f78:
    bad.append('f78.py still checks a.endswith("history=0")')
if not re.search(r'"history=0"\s+in\s+a\.split\(\)', f78):
    bad.append('f78.py must check `"history=0" in a.split()` for Close Panel')
if 'a.startswith("OK")' not in f78.split("Close Panel hides the panel")[1].split("\n")[0] + f78.split("Close Panel hides the panel")[0].split("\n")[-1]:
    pass
if 'p == "OK waveform=1 spectral=1 history=0"' in f78:
    bad.append("f78.py still compares the whole PANELS line")
run2 = f78.split("def run2(c):")[1].split("\n\n")[0] if "def run2(c):" in f78 else ""
if "p = panels(c)" not in run2:
    bad.append("run2 must read `p = panels(c)`")
for key, val in (("history", "0"), ("waveform", "1"), ("spectral", "1")):
    if f'p.get("{key}") == "{val}"' not in run2:
        bad.append(f'run2 must check p.get("{key}") == "{val}"')
try:
    compile(f78, "tools/alpha/f78.py", "exec")
except SyntaxError as e:
    bad.append(f"f78.py does not compile: {e}")
for b in bad:
    print("FAIL:", b)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] || exit 1
cargo check -p wavcraft -q || { echo "FAIL: cargo check -p wavcraft"; exit 1; }
echo "PASS: #983 locked check"
