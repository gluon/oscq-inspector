#!/usr/bin/env bash
# Fails if any tracked file contains a git conflict marker at the start of a line.
set -uo pipefail
git grep -n -I -E '^(<<<<<<< |\|\|\|\|\|\|\| |>>>>>>> |=======$)' -- .
case $? in
    0) echo "FAIL: conflict markers found in the tracked files above"; exit 1 ;;
    1) echo "ok: no conflict markers in tracked files"; exit 0 ;;
    *) echo "FAIL: git grep could not run"; exit 1 ;;
esac
