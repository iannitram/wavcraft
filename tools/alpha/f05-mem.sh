#!/bin/bash
# f05 before/after, the app itself: footprint and resident size on one synthetic long file,
# the tree at origin/main (whole-file copy) against this tree. One queued command.
set -e
# The two trees share one target dir (same relative paths): touching the sources forces a real
# rebuild of each, instead of cargo reusing the other tree's artifacts.
WT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE="${WT}-base"
SCR=~/dev/craft/_wt/scratch/f05mem
MIN="${1:-60}"
rm -rf "$SCR"; mkdir -p "$SCR"
python3 -c "
import importlib.util
spec = importlib.util.spec_from_file_location('f05', '$WT/tools/alpha/f05.py'); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
m.write_wav('$SCR/long.wav', $MIN)"
if [ ! -d "$BASE" ]; then git -C ~/dev/craft/wavcraft worktree add --detach "$BASE" origin/main; fi
(cd "$BASE" && git checkout -q --detach origin/main && find crates apps -name "*.rs" -exec touch {} + && cargo build --release -p wavcraft && cp "$CARGO_TARGET_DIR/release/wavcraft" "$SCR/app-before")
(cd "$WT" && find crates apps -name "*.rs" -exec touch {} + && cargo build --release -p wavcraft && cp "$CARGO_TARGET_DIR/release/wavcraft" "$SCR/app-after")
echo "=== before (origin/main) ==="
python3 "$WT/tools/alpha/f05-mem.py" --exe "$SCR/app-before" --wav "$SCR/long.wav" 2>&1 | tail -3 || true
echo "=== after ==="
python3 "$WT/tools/alpha/f05-mem.py" --exe "$SCR/app-after" --wav "$SCR/long.wav" 2>&1 | tail -3
rm -rf "$SCR"
