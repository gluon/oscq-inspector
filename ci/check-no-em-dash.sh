#!/usr/bin/env bash
# Fails if any tracked file contains an em dash (U+2014).
set -uo pipefail
git grep -n -I $'\xe2\x80\x94' -- .
case $? in
    0) echo "FAIL: em dash found in the tracked files above"; exit 1 ;;
    1) echo "ok: no em dash in tracked files"; exit 0 ;;
    *) echo "FAIL: git grep could not run"; exit 1 ;;
esac
