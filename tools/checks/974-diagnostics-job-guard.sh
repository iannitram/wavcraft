#!/usr/bin/env bash
# Locked check for #974: DeClipper, DeClicker / Automatic Click Remover and the Diagnostics repair
# refuse while a background job runs (so DECLIP, DECLICK, AUTOCLICK and `DIAG repair` answer
# `ERR wait for the background job to finish, or cancel it`), and the Diagnostics window's buttons
# are disabled during a job. The app's unit-test target does not build and the control channel needs
# a window, so this reads the source of each function. Run from the repo root.
set -u
fail=0
no() { echo "FAIL: $1"; fail=1; }
python3 -I - <<'PY' || fail=1
import re, sys
src = open("apps/wavcraft/src/app.rs", encoding="utf-8").read()
bad = 0
def no(msg):
    global bad
    print("FAIL: " + msg); bad = 1

def body(name):
    m = re.search(r'^    fn ' + name + r'\b.*?\{\n(.*?)^    \}\n', src, re.S | re.M)
    if not m:
        no("fn %s not found in app.rs" % name)
        return None
    return m.group(1)

GUARD = re.compile(r'if self\.running\.is_some\(\) \{\s*return Err\("wait for the background job to finish, or cancel it"\.to_string\(\)\);\s*\}')
SAVE = re.compile(r'if self\.saving\.is_some\(\) \{\s*return Err\("wait for the save to finish"\.to_string\(\)\);\s*\}')
for name, first_use in (("run_declip", r"self\.selection_range\(\)"), ("run_declick", r"self\.selection_range\(\)"), ("diag_repair", r"self\.editor\.as_mut\(\)")):
    b = body(name)
    if b is None:
        continue
    g, s, u = GUARD.search(b), SAVE.search(b), re.search(first_use, b)
    if not s:
        no("%s lost its save guard" % name)
    if not g:
        no("%s must refuse with: if self.running.is_some() { return Err(\"wait for the background job to finish, or cancel it\".to_string()); }" % name)
    elif s and u and not (s.start() < g.start() < u.start()):
        no("%s: the running guard must come after the save guard and before the edit starts" % name)
    if b.count("self.running.is_some()") != 1:
        no("%s should check self.running.is_some() exactly once" % name)

# The window: Scan and Repair buttons are disabled while a job runs.
b = body("diagnostics_ui")
if b is not None:
    m = re.search(r'let busy = ([^;]*);', b)
    if not m or not all(x in m.group(1) for x in ("self.saving.is_some()", "self.rebuilding.is_some()", "self.running.is_some()")):
        no("diagnostics_ui: `busy` must include self.saving, self.rebuilding and self.running (found: %s)" % (m.group(0) if m else "none"))

# Scanning only reads: diag_scan keeps no job guard.
b = body("diag_scan")
if b is not None and "self.running" in b:
    no("diag_scan is read-only and must not check self.running")

# The control verbs still reach the guarded functions and pass their error through as `ERR <why>`.
for verb, call in (("DECLIP", "self.run_declip(params, ctx)"), ("DECLICK", "self.run_declick(params, false, ctx)"), ("AUTOCLICK", "self.run_declick(params, true, ctx)"), ("DIAG", "self.diag_repair(&rows, ctx)")):
    m = re.search(r'^            "' + verb + r'" =>(.*?)(?=^            ["/}])', src, re.S | re.M)
    if not m or call not in m.group(1) or 'Err(e) => format!("ERR {e}")' not in m.group(1):
        no("the %s verb no longer calls %s and answers its error as ERR {e}" % (verb, call))
sys.exit(bad)
PY
# Scope: app.rs only, about 15 lines, no new warnings.
[ "$(git diff --name-only HEAD | grep -v '^tools/checks/974-')" = "apps/wavcraft/src/app.rs" ] || no "only apps/wavcraft/src/app.rs may change"
n=$(git diff --numstat HEAD -- apps/wavcraft/src/app.rs | awk '{print $1+$2}')
[ "${n:-0}" -le 30 ] || no "app.rs changed $n lines; this fix is about 15"
cargo check -p wavcraft -q 2>&1 | grep -q . && no "cargo check -p wavcraft prints output (error or warning)"

[ $fail -eq 0 ] && echo "PASS: #974 locked check"
exit $fail
