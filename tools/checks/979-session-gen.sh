#!/usr/bin/env bash
# Locked check for #979: a pending clip edit remembers its session; Return refuses after the session changed.
# Run from the repo root: bash tools/checks/979-session-gen.sh
set -u
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY' || exit 1
import re, sys
mt = open("apps/wavcraft/src/multitrack_ui.rs", encoding="utf-8").read()
app = open("apps/wavcraft/src/app.rs", encoding="utf-8").read()
bad = []

def body(src, head):
    """The text of the item whose header contains `head`, through its matching close brace."""
    i = src.find(head)
    if i < 0:
        bad.append(f"missing: {head}")
        return ""
    j = src.index("{", i)
    depth = 0
    for k in range(j, len(src)):
        depth += (src[k] == "{") - (src[k] == "}")
        if depth == 0:
            return src[i:k + 1]
    return src[i:]

def squash(s):
    return re.sub(r"\s+", " ", s)

view = body(mt, "pub struct MtView")
if not re.search(r"\bsession_gen: u64,", view):
    bad.append("MtView has no private `session_gen: u64` field")
if not re.search(r"\bsession_gen: 0,", body(mt, "impl Default for MtView")):
    bad.append("MtView::default does not set `session_gen: 0`")
if "pub fn session_gen(&self) -> u64 { self.session_gen }" not in squash(mt):
    bad.append("MtView has no `pub fn session_gen(&self) -> u64 { self.session_gen }`")
bump = "self.session_gen = self.session_gen.wrapping_add(1);"
if mt.count(bump) != 2:
    bad.append(f"`{bump}` must appear exactly twice in multitrack_ui.rs, found {mt.count(bump)}")
cw = body(mt, "fn create_with_master")
m = re.search(r"self\.mt = Some\(Multitrack::new_with_master\([^;]*;\s*" + re.escape(bump), cw)
if not m:
    bad.append("create_with_master must bump session_gen right after `self.mt = Some(Multitrack::new_with_master(...)?);`")
osn = body(mt, "pub fn open_session(")
ok_arm, _, err_arm = osn.partition("Err(e) =>")
if not re.search(r"self\.mt = Some\(mt\);\s*" + re.escape(bump), ok_arm):
    bad.append("open_session must bump session_gen right after `self.mt = Some(mt);` in the Ok arm")
if bump in err_arm:
    bad.append("open_session must not bump session_gen on failure")

if not re.search(r"\bsession_gen: u64,", body(app, "struct ClipEdit")):
    bad.append("ClipEdit has no `session_gen: u64` field")
if "session_gen: self.mtv.session_gen()" not in squash(body(app, "fn edit_clip_source")):
    bad.append("edit_clip_source must build ClipEdit with `session_gen: self.mtv.session_gen()`")

ret = body(app, "fn return_to_session")
chk = ret.find("edit.session_gen != self.mtv.session_gen()")
rep = ret.find("self.mtv.replace_source(")
if chk < 0:
    bad.append("return_to_session has no `edit.session_gen != self.mtv.session_gen()` check")
elif rep < 0 or chk > rep:
    bad.append("the session_gen check must come before self.mtv.replace_source(")
else:
    blk = body(ret[chk:], "{") if False else ret[chk:rep]
    msg = "the session changed since the clip source was opened: nothing was replaced; the edited audio is still open, Save As keeps it"
    if msg not in blk:
        bad.append("the check must return Err with the exact message: " + msg)
    if "remove_dir_all" in blk:
        bad.append("the check must not remove edit.dir (the open document's file is there)")
    if "self.clip_edit = Some" in blk:
        bad.append("the check must leave clip_edit as None")
    if "return Err(" not in blk:
        bad.append("the check must `return Err(...)` before replace_source")
if "self.mtv.replace_source(edit.source" not in ret:
    bad.append("the same-session path must still call replace_source(edit.source, ..)")

for b in bad:
    print("FAIL:", b)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] || exit 1
cargo check -p wavcraft -q || { echo "FAIL: cargo check -p wavcraft"; exit 1; }
echo "PASS: #979 locked check"
