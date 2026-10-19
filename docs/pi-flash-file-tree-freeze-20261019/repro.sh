#!/usr/bin/env bash
# pi-flash file-tree freeze repro
# Usage: bash repro.sh /path/to/project
#
# Demonstrates the difference between:
#   git status --untracked-files=all              (whole repo; can be 100 s / 400 MB)
#   git status --untracked-files=all -- .         (cwd only; can be 0.1 s / 300 KB)
#
# WARNING: the unscoped command can take minutes and produce hundreds of MB.
set -u

PROJECT="${1:-}"
if [ -z "$PROJECT" ]; then
  echo "usage: bash repro.sh /path/to/project" >&2
  exit 2
fi

cd "$PROJECT" || exit 1

echo "cwd:       $(pwd)"
echo "repo_root: $(git rev-parse --show-toplevel 2>/dev/null || echo '<not a git repo>')"
echo

echo "=== 1) unscoped: git status -uall (may take minutes / hundreds of MB) ==="
time git status --porcelain=v1 -z --untracked-files=all > /tmp/pi-flash-status-unscoped.bin
echo "bytes:   $(wc -c < /tmp/pi-flash-status-unscoped.bin)"
echo "records: $(tr -cd '\0' < /tmp/pi-flash-status-unscoped.bin | wc -c)"
echo

echo "=== 2) scoped: git status -uall -- . ==="
time git status --porcelain=v1 -z --untracked-files=all -- . > /tmp/pi-flash-status-scoped.bin
echo "bytes:   $(wc -c < /tmp/pi-flash-status-scoped.bin)"
echo "records: $(tr -cd '\0' < /tmp/pi-flash-status-scoped.bin | wc -c)"
echo

echo "=== 3) project full walk (for comparison) ==="
python - "$PWD" <<'PY'
import os, sys, time
root = sys.argv[1]
t = time.time()
files = dirs = 0
for _, dn, fn in os.walk(root):
    files += len(fn)
    dirs += len(dn)
print(f"files={files} dirs={dirs} walk_sec={time.time() - t:.3f}")
PY

echo
echo "cleanup: rm -f /tmp/pi-flash-status-unscoped.bin /tmp/pi-flash-status-scoped.bin"
