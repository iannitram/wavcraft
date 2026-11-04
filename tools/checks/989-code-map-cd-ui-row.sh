#!/usr/bin/env bash
# Locked check for #989: the cd_ui.rs row (and the input_meter.rs row, also 3-cell) are in the first code-map table, after mixer.rs;
# "Also on main" has 2-cell rows only.
# Run from the repo root: bash tools/checks/989-code-map-cd-ui-row.sh
set -u
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY'
import re, sys
lines = open("docs/code-map.md").read().split("\n")
bad = []
hits = [i for i, l in enumerate(lines) if "cd_ui.rs" in l]
meter = [i for i, l in enumerate(lines) if "`input_meter.rs`" in l]
mixer = [i for i, l in enumerate(lines) if l.startswith("| | `mixer.rs`")]
also = [i for i, l in enumerate(lines) if l.startswith("### Also on main")]
if len(hits) != 1:
    bad.append(f"cd_ui.rs appears on {len(hits)} lines, want 1")
if len(meter) != 1:
    bad.append(f"input_meter.rs appears on {len(meter)} lines, want 1")
if len(mixer) != 1 or len(also) != 1:
    bad.append("the mixer.rs row or the `### Also on main` heading is missing")
if not bad:
    if hits[0] != mixer[0] + 1:
        bad.append(f"the cd_ui.rs row must be the line right after the mixer.rs row (mixer line {mixer[0] + 1}, cd_ui line {hits[0] + 1})")
    if not lines[hits[0]].startswith("| | `cd_ui.rs` (f121) | The CD Layout view"):
        bad.append("the cd_ui.rs row text changed")
    if meter[0] != hits[0] + 1 or not lines[meter[0]].startswith("| | `input_meter.rs` (f56) | The input meter"):
        bad.append("the input_meter.rs row must be the line right after the cd_ui.rs row, text unchanged")
    elif lines[meter[0] + 1].strip() != "" or meter[0] + 1 >= also[0]:
        bad.append("the moved rows must end the first table (a blank line follows them)")
    rest = lines[also[0]:]
    first = next(i for i, l in enumerate(rest) if l.startswith("|"))
    table = []
    for l in rest[first:]:
        if not l.startswith("|"):
            break
        table.append(l)
    for l in table:
        cells = re.split(r"(?<!\\)\|", l.strip())[1:-1]
        if len(cells) != 2:
            bad.append(f"a row in `Also on main` has {len(cells)} cells: {l[:70]}")
    if len(table) < 3 or not table[-1].startswith("| `apps/wavcraft/src/input_monitor.rs`"):
        bad.append("`Also on main` must end with the input_monitor.rs row")
for b in bad:
    print("FAIL:", b)
print("FAIL: #989 locked check" if bad else "PASS: #989 locked check")
sys.exit(1 if bad else 0)
PY
