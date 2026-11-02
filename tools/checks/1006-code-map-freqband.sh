#!/usr/bin/env bash
# Locked check for #1006: code-map.md and the spectral_view doc comment name the STATE keys and
# STATUSBAR verbs the code uses. Docs and one doc comment only; no code changes.
# Run from the repo root: bash tools/checks/1006-code-map-freqband.sh
set -u
fail=0
bad() { echo "FAIL: $1"; fail=1; }
map=docs/code-map.md
# Table cells escape the pipes: `lin\|log_partial\|log`.
grep -qF '`STATE` shows `freqband=<lo>-<hi>:lin\|log_partial\|log`' $map || bad "code-map.md does not say STATE shows freqband=<lo>-<hi>:lin\\|log_partial\\|log"
grep -qF 'spectral=<lo' $map && bad "code-map.md still says spectral=<lo>-<hi>"
grep -qF '`statusbar=<fields>`' $map || bad "code-map.md does not say STATE shows statusbar=<fields>"
grep -F '`statusbar.rs` (f52)' $map | grep -qF '`STATUSBAR show\|hide <field> \| list`' || bad "the statusbar.rs row does not list STATUSBAR show\\|hide <field> \\| list"
grep -qF '(`spectral_view = <lo_hz> <hi_hz> lin|log_partial|log`);' crates/core/src/settings.rs || bad "the spectral_view doc comment does not read: spectral_view = <lo_hz> <hi_hz> lin|log_partial|log"
grep -qF '<hi_hz> log|lin' crates/core/src/settings.rs && bad "settings.rs still says log|lin for spectral_view"
# Nothing else changed: only these two files, and in settings.rs only comment lines.
base=$(git merge-base HEAD origin/main 2>/dev/null || echo HEAD)
others=$(git diff --name-only "$base" -- . ':!docs/code-map.md' ':!crates/core/src/settings.rs' ':!tools/checks')
[ -z "$others" ] || bad "files other than code-map.md and settings.rs changed: $others"
code=$(git diff -U0 "$base" -- crates/core/src/settings.rs | grep -E '^[+-]' | grep -vE '^(\+\+\+|---)' | grep -vE '^[+-][[:space:]]*//')
[ -z "$code" ] || bad "settings.rs changed outside comments: $code"
[ $fail -eq 0 ] && echo "PASS: #1006 locked check"
exit $fail
