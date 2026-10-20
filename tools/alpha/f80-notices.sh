#!/bin/bash
# Generates and checks THIRD-PARTY-NOTICES.txt (cargo metadata is cargo, so this runs queued).
set -e
python3 tools/notices.py --write
python3 tools/notices.py --check
