#!/usr/bin/env bash
# Locked check for #980: a click on a clip box selects that clip (sel_clip), and the rest is unchanged.
# Run from the repo root: bash tools/checks/980-click-selects-clip.sh
set -u
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY' || exit 1
import re, sys
src = open("apps/wavcraft/src/multitrack_ui.rs").read()
bad = []

def block(text, head):
    i = text.find(head)
    if i < 0:
        return None
    j = text.index("{", i)
    depth = 0
    for k in range(j, len(text)):
        depth += (text[k] == "{") - (text[k] == "}")
        if depth == 0:
            return text[j:k + 1]
    return None

clip_click = block(src, "if cresp.clicked()")
if clip_click is None:
    bad.append("no `if cresp.clicked() {` block in multitrack_ui.rs")
elif "self.sel_clip = Some((id, c.id));" not in clip_click:
    bad.append("`if cresp.clicked()` must set `self.sel_clip = Some((id, c.id));`")
elif "self.selected = Some(id);" not in clip_click:
    bad.append("`if cresp.clicked()` must still set `self.selected = Some(id);`")
lane = block(src, "if lane_resp.clicked()")
if lane is None or "self.sel_clip = lane_resp.interact_pointer_pos()" not in lane:
    bad.append("the lane click handler must still set sel_clip from the pointer position (an empty lane clears it)")
if src.count("self.sel_clip = Some((id, c.id));") != 1:
    bad.append("`self.sel_clip = Some((id, c.id));` must appear exactly once")
dbl = block(src, "if cresp.double_clicked()")
if dbl is None or "self.edit_request = Some((id, ci + 1));" not in dbl:
    bad.append("double-click must still set edit_request = Some((id, ci + 1))")
if '"Edit in Waveform editor"' not in src:
    bad.append("the right-click `Edit in Waveform editor` menu must stay")
for b in bad:
    print("FAIL:", b)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] || exit 1
cargo check -p wavcraft -q || { echo "FAIL: cargo check -p wavcraft"; exit 1; }
echo "PASS: #980 locked check"
