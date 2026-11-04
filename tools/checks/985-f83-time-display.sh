#!/usr/bin/env bash
# Locked check for #985: tools/alpha/f83.py uses time_display and no longer names time_format or frame_rate.
# Run from the repo root: bash tools/checks/985-f83-time-display.sh
set -u
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY' || exit 1
import ast, sys
path = "tools/alpha/f83.py"
src = open(path).read()
bad = []
for old in ("time_format", "frame_rate"):
    if old in src:
        bad.append(f"{path} still mentions {old}")
vals = {}
for node in ast.parse(src).body:
    if isinstance(node, ast.Assign) and isinstance(node.targets[0], ast.Name) and node.targets[0].id in ("DEFAULTS", "CHANGED", "BAD"):
        vals[node.targets[0].id] = ast.literal_eval(node.value)
D = {"spec_window": "hann", "spec_resolution": "2048", "spec_db_range": "100", "spec_play_selected_freqs": "false", "time_display": "decimal",
     "loudness_target_lufs": "-16", "true_peak_ceiling_dbtp": "-1", "zoom_factor": "2", "extend_selection": "nearest",
     "context_channel_editing": "true", "theme": "dark", "waveform_colour": "green", "ui_brightness": "1"}
C = {"spec_window": "blackman-harris", "spec_resolution": "4096", "spec_db_range": "120", "spec_play_selected_freqs": "true",
     "time_display": "smpte_2997df", "loudness_target_lufs": "-23", "true_peak_ceiling_dbtp": "-2.5", "zoom_factor": "3",
     "extend_selection": "anchor", "context_channel_editing": "false", "theme": "light", "waveform_colour": "amber", "ui_brightness": "0.8"}
B = [("spec_window", "triangle"), ("spec_resolution", "1000"), ("spec_db_range", "161"), ("spec_play_selected_freqs", "yes"),
     ("time_display", "hex"), ("loudness_target_lufs", "1"), ("true_peak_ceiling_dbtp", "0.5"), ("zoom_factor", "1"),
     ("extend_selection", "left"), ("context_channel_editing", "1"), ("theme", "sepia"), ("waveform_colour", "#ff0000"), ("ui_brightness", "inf")]
if vals.get("DEFAULTS") != D:
    bad.append(f"DEFAULTS is {vals.get('DEFAULTS')}")
if vals.get("CHANGED") != C:
    bad.append(f"CHANGED is {vals.get('CHANGED')}")
if vals.get("BAD") != B:
    bad.append(f"BAD is {vals.get('BAD')}")
for b in bad:
    print("FAIL:", b)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] || exit 1
python3 -m py_compile tools/alpha/f83.py || { echo "FAIL: py_compile"; exit 1; }
echo "PASS: #985 locked check"
