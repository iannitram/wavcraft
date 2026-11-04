#!/usr/bin/env bash
# Locked check for #973: the DELETE_SILENCE control arm refuses while a background job runs, with
# the same reply STRETCH and run_action give, after the save check and before any edit. The app's
# unit-test target does not build and the control channel needs a window, so this reads the arm's
# source. Run from the repo root.
set -u
fail=0
no() { echo "FAIL: $1"; fail=1; }
python3 -I - <<'PY' || fail=1
import re, sys
src = open("apps/wavcraft/src/app.rs", encoding="utf-8").read()
m = re.search(r'^( *)"DELETE_SILENCE" => \{\n(.*?)^\1\}\n', src, re.S | re.M)
if not m:
    print('FAIL: the "DELETE_SILENCE" arm of control_request was not found'); sys.exit(1)
arm = m.group(2)
bad = 0
def at(pat):
    k = re.search(pat, arm)
    return k.start() if k else -1
def no(msg):
    global bad
    print("FAIL: " + msg); bad = 1

parse = at(r'cannot use \{kv:\?\}')
save = at(r'self\.saving\.is_some\(\)')
run = at(r'self\.running\.is_some\(\)')
sel = at(r'self\.selection_range\(\)')
edit = at(r'delete_silence\(')
reply = '"ERR wait for the background job to finish, or cancel it".to_string()'
if run < 0:
    no("the DELETE_SILENCE arm never checks self.running.is_some()")
if arm.count("self.running.is_some()") != 1:
    no("the arm should check self.running.is_some() exactly once, found %d" % arm.count("self.running.is_some()"))
if not re.search(r'if self\.running\.is_some\(\) \{\s*return ' + re.escape(reply) + r';\s*\}', arm):
    no("the guard must be: if self.running.is_some() { return %s; }" % reply)
if save < 0 or '"ERR wait for the save to finish".to_string()' not in arm:
    no("the save guard and its reply 'ERR wait for the save to finish' must stay")
if min(parse, save, sel, edit) < 0:
    no("the arm lost its settings parse, save guard, selection or delete_silence call")
elif run >= 0:
    if not (parse < save < run < sel < edit):
        no("order must be: settings parse, save check, running check, then selection/edit (parse=%d save=%d run=%d sel=%d edit=%d)" % (parse, save, run, sel, edit))
for keep in ('ERR no file is open', 'ERR Delete Silence: {e}', 'OK stretches={} removed_s={:.3}'):
    if keep not in arm:
        no("the arm lost its reply text %r" % keep)
sys.exit(bad)
PY
# The STRETCH arm is the pattern and stays as it is: its guard text is still in the file in both arms.
[ "$(grep -c '"ERR wait for the background job to finish, or cancel it".to_string()' apps/wavcraft/src/app.rs)" -ge 2 ] || no "app.rs should hold the background-job reply in the STRETCH arm and the DELETE_SILENCE arm"
# Scope: app.rs only, a handful of lines, no new warnings.
[ "$(git diff --name-only HEAD | grep -v '^tools/checks/973-')" = "apps/wavcraft/src/app.rs" ] || no "only apps/wavcraft/src/app.rs may change"
n=$(git diff --numstat HEAD -- apps/wavcraft/src/app.rs | awk '{print $1+$2}')
[ "${n:-0}" -le 10 ] || no "app.rs changed $n lines; this fix is about 3"
cargo check -p wavcraft -q 2>&1 | grep -q . && no "cargo check -p wavcraft prints output (error or warning)"

[ $fail -eq 0 ] && echo "PASS: #973 locked check"
exit $fail
