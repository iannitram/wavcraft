#!/bin/bash
# f05 before/after: peak resident memory and times of the headless bench on one synthetic
# long file, the tree at origin/main (whole-file copy) against this tree (reads from the store).
set -e
# The two trees share one target dir (same relative paths): touching the sources forces a real
# rebuild of each, instead of cargo reusing the other tree's artifacts.
WT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE="${WT}-base"
SCR=~/dev/craft/_wt/scratch/f05bench
MIN="${1:-60}"
rm -rf "$SCR"; mkdir -p "$SCR/home"
python3 -c "
import importlib.util, sys
spec = importlib.util.spec_from_file_location('f05', '$WT/tools/alpha/f05.py'); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
m.write_wav('$SCR/long.wav', $MIN)"
if [ ! -d "$BASE" ]; then git -C ~/dev/craft/wavcraft worktree add --detach "$BASE" origin/main; fi
(cd "$BASE" && git checkout -q --detach origin/main && find crates apps -name "*.rs" -exec touch {} + && cargo build --release -p wavcraft-testkit --bin wavcraft-bench && cp "$CARGO_TARGET_DIR/release/wavcraft-bench" "$SCR/bench-before")
(cd "$WT" && find crates apps -name "*.rs" -exec touch {} + && cargo build --release -p wavcraft-testkit --bin wavcraft-bench && cp "$CARGO_TARGET_DIR/release/wavcraft-bench" "$SCR/bench-after")
for which in before after; do
  echo "=== $which ==="
  HOME="$SCR/home" /usr/bin/time -l "$SCR/bench-$which" "$SCR/long.wav" 2>&1 | grep -E "working set|opened|open \(|peaks build|overview build|picture, whole file, 2048|full ordered|maximum resident|peak memory footprint|real"
done
rm -rf "$SCR"
