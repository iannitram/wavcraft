#!/usr/bin/env bash
# Locked check for #995: no compiled Python cache is tracked, and .gitignore ignores it.
# Run from the repo root: bash tools/checks/995-pycache.sh
set -u
fail=0
tracked=$(git ls-files | grep -E '\.pyc$|__pycache__')
if [ -n "$tracked" ]; then echo "FAIL: compiled Python is tracked: $tracked"; fail=1; fi
if ! grep -qx '__pycache__/' .gitignore; then echo "FAIL: .gitignore has no __pycache__/ line"; fail=1; fi
if ! grep -qx '\*\.pyc' .gitignore; then echo "FAIL: .gitignore has no *.pyc line"; fail=1; fi
for old in '/target' '/target-*/' '*~' '.DS_Store' 'tools/winlist'; do
  grep -qxF -e "$old" .gitignore || { echo "FAIL: .gitignore lost the line $old"; fail=1; }
done
if [ -n "$(tr -cd '\r' < .gitignore)" ]; then echo "FAIL: .gitignore has carriage returns"; fail=1; fi
# the ignore rules work: a cache file written under tools/ would not show in git status
mkdir -p tools/__pycache__ && : > tools/__pycache__/probe.cpython-314.pyc
if git status --porcelain --untracked-files=all | grep -q 'probe.cpython-314.pyc'; then echo "FAIL: a new .pyc under tools/__pycache__ still shows in git status"; fail=1; fi
rm -f tools/__pycache__/probe.cpython-314.pyc
# nothing else changed: only .gitignore and the removed .pyc show in git status
others=$(git status --porcelain | grep -v -e '\.gitignore$' -e '\.pyc$' -e 'tools/checks/')
if [ -n "$others" ]; then echo "FAIL: other changes: $others"; fail=1; fi
[ $fail -eq 0 ] && echo "PASS: #995 locked check"
exit $fail
