#!/usr/bin/env bash
# Locked check for #975: Control.state() in tools/alpha/f32.py and f03.py splits the STATE answer
# on the trailing ` path=` first, so redo is the bare label and path holds the document path.
# The app also emits ` band=` between redo= and path=; the band cases below cover that shape.
# (The scripts need a built app, so each state() is lifted out of its file and fed canned answers.)
# Run from the repo root.
set -u
python3 -I - <<'PY'
import ast, sys, time

HEAD = "OK frames={fr} channels=2 rate=48000 selection=0-0 steps=1 cursor=0 busy={busy} progress=0 unsaved=1 tool=select view=0-240000 time_display=decimal selection_text=- snap_frames=1"
CASES = {
    "a document is open: redo is the bare label and path keeps its spaces": (
        HEAD.format(fr=240000, busy=0) + " undo=- redo=Delete path=/work/my take.wav",
        dict(frames="240000", busy="0", undo="-", redo="Delete", path="/work/my take.wav")),
    "no document: undo and redo are - and path is empty": (
        HEAD.format(fr=0, busy=0).replace("channels=2 rate=48000", "channels=0 rate=0") + " undo=- redo=- path=",
        dict(frames="0", busy="0", undo="-", redo="-", path="")),
    "an undo label with spaces still parses": (
        HEAD.format(fr=240000, busy=0) + " undo=Delete Silence redo=- path=/a.wav",
        dict(undo="Delete Silence", redo="-", path="/a.wav")),
    "an undo label and a redo label, both with spaces": (
        HEAD.format(fr=240000, busy=0) + " undo=Fade In (Linear) redo=Delete Silence path=/x y/z.wav",
        dict(undo="Fade In (Linear)", redo="Delete Silence", path="/x y/z.wav")),
    "a document is open with band=: redo is the bare label, band and path are split": (
        HEAD.format(fr=240000, busy=0) + " undo=- redo=Delete band=none path=/work/my take.wav",
        dict(redo="Delete", band="none", path="/work/my take.wav")),
    "a redo label with spaces before band=": (
        HEAD.format(fr=240000, busy=0) + " undo=Fade In (Linear) redo=Delete Silence band=100-2000 path=/x y/z.wav",
        dict(undo="Fade In (Linear)", redo="Delete Silence", band="100-2000", path="/x y/z.wav")),
    "no document with band=: band is none and path is empty": (
        HEAD.format(fr=0, busy=0).replace("channels=2 rate=48000", "channels=0 rate=0") + " undo=- redo=- band=none path=",
        dict(undo="-", redo="-", band="none", path="")),
}
bad = 0
for fname in ("tools/alpha/f32.py", "tools/alpha/f03.py"):
    src = open(fname, encoding="utf-8").read()
    tree = ast.parse(src)
    fn = next((n for c in tree.body if isinstance(c, ast.ClassDef) and c.name == "Control" for n in c.body if isinstance(n, ast.FunctionDef) and n.name == "state"), None)
    if fn is None:
        print(f"FAIL: {fname}: Control.state() not found"); bad = 1; continue
    ns = {"time": time, "shutil": __import__("shutil")}
    exec(compile("class C:\n" + ast.get_source_segment(src, fn, padded=True), fname, "exec"), ns)
    for name, (answer, want) in CASES.items():
        c = ns["C"]()
        c.ask = lambda line, a=answer: a
        try:
            got = c.state()
        except Exception as e:
            print(f"FAIL: {fname}: {name}: raised {e!r}"); bad = 1; continue
        wrong = {k: (got.get(k), v) for k, v in want.items() if got.get(k) != v}
        if wrong:
            print(f"FAIL: {fname}: {name}: (got, want) {wrong}"); bad = 1
    # A busy answer is retried, and the parsed fields come from the idle one.
    answers = [HEAD.format(fr=240000, busy=1) + " undo=- redo=- path=/a.wav", HEAD.format(fr=240000, busy=0) + " undo=Delete redo=- path=/a.wav"]
    c = ns["C"](); c.ask = lambda line: answers.pop(0) if len(answers) > 1 else answers[0]
    real_sleep, time.sleep = time.sleep, (lambda s: None)
    try:
        got = c.state()
    finally:
        time.sleep = real_sleep
    if (got.get("busy"), got.get("undo"), got.get("redo"), got.get("path")) != ("0", "Delete", "-", "/a.wav"):
        print(f"FAIL: {fname}: a busy answer should be retried and the idle one parsed, got {got}"); bad = 1
if not bad:
    print("PASS: #975 locked check")
sys.exit(bad)
PY
rc=$?
# Only the alpha scripts change: not the app, golden.py or f43.py.
extra=$(git diff --name-only HEAD | grep -v '^tools/checks/975-' | grep -vE '^tools/alpha/(f32|f03|f38|f88)\.py$')
[ -z "$extra" ] || { echo "$extra"; echo "FAIL: files outside f32.py, f03.py, f38.py and f88.py changed"; rc=1; }
exit $rc
