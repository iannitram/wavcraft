#!/usr/bin/env bash
# Locked check for #996: the f26 doc comment sits on its own test, and set_map keeps only its own comment.
# Run from the repo root: bash tools/checks/996-f26-doc-comment.sh
set -u
fail=0
f=apps/wavcraft/src/player.rs
f26='/// f26: what the callback sends is what the Levels panel gets, per channel, without a lock.'
setmap='/// Set the channel map a play is rendered through (the text form, as the settings hold it).'
trim() { sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'; }
# the comment appears once, and the two lines after it are #[test] and the callback test
if [ "$(grep -cF -e "$f26" "$f")" != "1" ]; then echo "FAIL: the f26 comment is not there exactly once"; fail=1; fi
after=$(grep -F -A2 -e "$f26" "$f" | tail -n 2 | trim | tr '\n' '|')
if [ "$after" != "#[test]|fn the_callback_records_the_peaks_it_sends() {|" ]; then echo "FAIL: after the f26 comment I found: $after"; fail=1; fi
# the line before set_map is its own doc comment, and the line before that is not the f26 comment
n=$(grep -n '^    fn set_map(shared: &Shared, text: &str) {' "$f" | head -1 | cut -d: -f1)
if [ -z "$n" ]; then echo "FAIL: fn set_map is missing"; fail=1; else
  prev=$(sed -n "$((n-1))p" "$f" | trim)
  prev2=$(sed -n "$((n-2))p" "$f" | trim)
  if [ "$prev" != "$setmap" ]; then echo "FAIL: set_map is preceded by: $prev"; fail=1; fi
  if [ "$prev2" = "$f26" ]; then echo "FAIL: the f26 comment still sits above set_map's comment"; fail=1; fi
fi
# a move only: the callback test, set_map and the two map tests are still there
for want in 'fn a_channel_map_routes_stereo_to_outputs_three_and_four()' 'fn a_map_sums_channels_sharing_an_output_and_drops_those_off_the_device()'; do
  grep -qF -e "$want" "$f" || { echo "FAIL: $want is gone"; fail=1; }
done
# no other file is touched
others=$(git status --porcelain | grep -v -e "$f" -e 'tools/checks/')
if [ -n "$others" ]; then echo "FAIL: other changes: $others"; fail=1; fi
# only the comment moved: the file differs from HEAD by nothing but that line
if ! git diff --quiet HEAD -- "$f"; then
  gone=$(git diff -U0 HEAD -- "$f" | grep -E '^-[^-]' | sed 's/^-//' | trim | sort)
  came=$(git diff -U0 HEAD -- "$f" | grep -E '^\+[^+]' | sed 's/^+//' | trim | sort)
  if [ "$gone" != "$came" ] || [ "$gone" != "$f26" ]; then echo "FAIL: more than the f26 line moved"; fail=1; fi
fi
[ $fail -eq 0 ] && echo "PASS: #996 locked check"
exit $fail
