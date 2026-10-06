#!/bin/bash
# Source-only project archive: excludes build output, packages and git history.
set -e
cd "$(dirname "$0")/.."
OUT=${1:-../tunebox-$(date +%Y%m%d).zip}
rm -f "$OUT"
zip -qr "$OUT" . -x 'target/*' 'dist/*' '.claude/worktrees/*' '.git/*' '*.swp' '.DS_Store'
echo "$OUT ($(du -h "$OUT" | cut -f1), $(unzip -l "$OUT" | tail -1 | awk '{print $2}') files)"
