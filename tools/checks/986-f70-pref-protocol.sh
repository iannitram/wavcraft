#!/usr/bin/env bash
# Locked check for #986: tools/alpha/f70.py speaks the generic PREF protocol (`PREF get <key>` -> `OK <value>`, `PREF set` -> `OK`).
# Run from the repo root: bash tools/checks/986-f70-pref-protocol.sh
set -u
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY' || exit 1
import ast, re, sys
path = "tools/alpha/f70.py"
src = open(path).read()
bad = []
KEYS = ("autosave_enabled", "autosave_interval_secs", "autosave_keep_days", "cache_mb")

if re.search(r'PREF get"', src):
    bad.append('a bare `PREF get` (no key) is still sent')
if re.search(r'kv\(c\.ask\(f?"PREF get', src):
    bad.append("kv(...) still parses a PREF get answer")
if "OK autosave_enabled=" in src:
    bad.append('PREF set is still expected to answer "OK autosave_enabled=..."')
for v in ("off", "on"):
    line = next((l for l in src.splitlines() if f'c.ask("PREF set autosave_enabled {v}")' in l and "check(" in l), "")
    if not re.search(r'==\s*"OK"\s*[,)]', line):
        bad.append(f"`PREF set autosave_enabled {v}` must be checked against exactly \"OK\"")
for need in ('c.ask("PREF set autosave_interval_secs 0").startswith("ERR")', 'c.ask("PREF set autosave_enabled maybe").startswith("ERR")'):
    if need not in src:
        bad.append("bad-value check lost: " + need)
holds = next((l for l in src.splitlines() if "settings.txt holds the keys" in l), "")
if "autosave_enabled = on" in holds:
    bad.append("settings.txt is not written with defaults: `autosave_enabled = on` must not be required")
for k in ("autosave_interval_secs = 6", "autosave_keep_days = 7", "cache_mb = 128"):
    if k not in holds:
        bad.append(f"settings.txt check lost `{k}`")
if not any("check(" in l and "text" in l and "autosave_enabled" in l and "holds the keys" not in l for l in src.splitlines()):
    bad.append("no check that autosave_enabled is absent from settings.txt or reads `autosave_enabled = on`")
if sum("prefs(c)" in l for l in src.splitlines()) < 2:
    bad.append("prefs(c) must replace both PREF get reads (run 1 and run 3)")
if "def kv(" not in src or src.count("kv(c.ask(") < 3:
    bad.append("kv must stay for the CACHE info / CACHE cleanup replies")

tree = ast.parse(src)
fn = next((n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "prefs"), None)
if fn is None:
    bad.append("no module-level `def prefs(c)`")
else:
    class Fake:
        def __init__(self): self.sent = []
        def ask(self, line):
            self.sent.append(line)
            return "OK " + {"autosave_enabled": "on", "autosave_interval_secs": "2", "autosave_keep_days": "30", "cache_mb": "default"}[line.split()[2]]
    ns = {}
    exec(compile(ast.Module([fn], []), path, "exec"), ns)
    c = Fake()
    got = ns["prefs"](c)
    if got != {"autosave_enabled": "on", "autosave_interval_secs": "2", "autosave_keep_days": "30", "cache_mb": "default"}:
        bad.append(f"prefs(c) returned {got}")
    if sorted(c.sent) != sorted(f"PREF get {k}" for k in KEYS):
        bad.append(f"prefs(c) must send one `PREF get <key>` per key, sent {c.sent}")
try:
    compile(src, path, "exec")
except SyntaxError as e:
    bad.append(f"does not compile: {e}")
for b in bad:
    print("FAIL:", b)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] || exit 1
python3 -m py_compile tools/alpha/f70.py || { echo "FAIL: py_compile"; exit 1; }
echo "PASS: #986 locked check"
