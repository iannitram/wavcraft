#!/usr/bin/env bash
# Locked check for #1008: the five clipboards AND the current-clipboard index move with the window
# across Open, Switch and Close. (The clips themselves are already carried on main; the index is not.)
# WaveApp cannot be built in a test, so this checks the three methods in the source; tools/alpha/f04.py
# and f12.py are the end-to-end checks and need the release app.
# Run from the repo root: bash tools/checks/1008-clipboards-belong-to-the-window.sh
set -u
fail=0
bad() { echo "FAIL: $1"; fail=1; }
app=apps/wavcraft/src/app.rs
body() { awk -v f="$1" '$0 ~ "^    fn " f "\\(" {on=1} on {print} on && /^    }$/ {exit}' $app; }

for fn in finish_opening switch_doc remove_current; do
  b=$(body $fn)
  [ -n "$b" ] || { bad "fn $fn is not in app.rs"; continue; }
  # The clips are taken out of the editor being left, and put into the one that takes over.
  printf '%s\n' "$b" | grep -qF 'std::mem::take(&mut e.clipboards)' || bad "fn $fn no longer takes the clipboards out of the editor"
  # The index travels with them: it is read from the old editor and written to the new one.
  [ "$(printf '%s\n' "$b" | grep -c 'current_clipboard')" -ge 2 ] || bad "fn $fn does not carry current_clipboard (read it from the old editor, write it to the new one)"
  printf '%s\n' "$b" | grep -qE 'current_clipboard\)? *= ' || bad "fn $fn never assigns current_clipboard to the editor that takes over"
done

# Nothing else changed: only app.rs, and the one-document paths keep their shape (Editor::new still starts at 0).
base=$(git merge-base HEAD origin/main 2>/dev/null || echo HEAD)
others=$(git diff --name-only "$base" -- . ':!tools/checks' ':!apps/wavcraft/src/app.rs')
[ -z "$others" ] || bad "files other than app.rs changed: $others"
grep -q $'\r' $app && bad "$app gained CR line endings"
grep -qF 'clipboards: Default::default(), current_clipboard: 0,' crates/core/src/actions.rs || bad "Editor::new changed"

cargo check --workspace -q || bad "cargo check --workspace"
[ $fail -eq 0 ] && echo "PASS: #1008 locked check"
exit $fail
