#!/usr/bin/env bash
# Locked check for #1001: the document's tempo is handed to the player's metronome when a document
# is loaded and when the tempo is set. WaveApp cannot be built in a test, so this checks the two call
# sites in the source; the end-to-end check is tools/alpha/f106.py ("the mixer follows the new tempo
# and meter"), which needs the release app and is run by the alpha queue.
# Run from the repo root: bash tools/checks/1001-metronome-follows-tempo.sh
set -u
fail=0
bad() { echo "FAIL: $1"; fail=1; }
app=apps/wavcraft/src/app.rs
call_open='self.player.set_metronome(self.player.metronome(), self.tempo);'
call_set='self.player.set_metronome(self.player.metronome(), tempo);'

# 1. The document-loaded block: the call comes right after `self.tempo = self.settings.tempo_for(&self.doc_path);`.
[ "$(grep -c 'self.tempo = self.settings.tempo_for(&self.doc_path);' $app)" = 1 ] || bad "the document-loaded tempo line is not there exactly once"
next=$(grep -A1 -F 'self.tempo = self.settings.tempo_for(&self.doc_path);' $app | sed -n 2p | sed 's/^[[:space:]]*//')
[ "$next" = "$call_open" ] || bad "the line after 'self.tempo = self.settings.tempo_for(&self.doc_path);' is not: $call_open (it is: $next)"

# 2. fn set_tempo: the call comes right after `self.tempo = tempo;` and before the empty-path early return.
body=$(awk '/^    fn set_tempo\(&mut self, tempo: Tempo\) \{/ {on=1} on {print} on && /^    }$/ {exit}' $app)
[ -n "$body" ] || bad "fn set_tempo is not in app.rs"
second=$(printf '%s\n' "$body" | sed -n 2p | sed 's/^[[:space:]]*//')
third=$(printf '%s\n' "$body" | sed -n 3p | sed 's/^[[:space:]]*//')
[ "$second" = "self.tempo = tempo;" ] || bad "fn set_tempo no longer starts with 'self.tempo = tempo;'"
[ "$third" = "$call_set" ] || bad "the line after 'self.tempo = tempo;' in fn set_tempo is not: $call_set (it is: $third)"
printf '%s\n' "$body" | grep -q 'is_empty()' || bad "fn set_tempo lost its empty-path early return"

# 3. The existing metronome-control path is unchanged, and nothing else moved: two added lines, none removed.
grep -qF 'self.player.set_metronome(m, self.tempo);' $app || bad "fn set_metronome (the control path) changed"
base=$(git merge-base HEAD origin/main 2>/dev/null || echo HEAD)
stat=$(git diff --numstat "$base" -- . ':!tools/checks')
[ "$(printf '%s\n' "$stat" | awk '{a+=$1; d+=$2} END {print a" "d}')" = "2 0" ] || bad "the change is not exactly two added lines: $stat"
[ "$(printf '%s\n' "$stat" | awk '{print $3}')" = "$app" ] || bad "files other than $app changed: $stat"
grep -q $'\r' $app && bad "$app gained CR line endings"

cargo check --workspace -q || bad "cargo check --workspace"
[ $fail -eq 0 ] && echo "PASS: #1001 locked check"
exit $fail
