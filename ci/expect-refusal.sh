#!/usr/bin/env bash
# Runs a command that must fail, and fail at one check: its exit status is not 0, and its one FAIL
# line, the first failure stopping it, holds the expected text. A failure at another check fails
# this script too. For the refusals of the workflow Accept artifact.
# Usage: expect-refusal.sh "EXPECTED" COMMAND [ARGUMENTS...]
set -uo pipefail
expected="${1:?usage: expect-refusal.sh EXPECTED COMMAND...}"
shift
out=$("$@" 2>&1)
status=$?
printf '%s\n' "$out"
if [ "$status" -eq 0 ]; then
    echo "FAIL: it passed; it should have failed with: $expected"
    exit 1
fi
failed=$(printf '%s\n' "$out" | grep '^FAIL: ' | head -1)
case "$failed" in
    *"$expected"*) echo "refused as expected, status $status: $failed" ;;
    *)
        echo "FAIL: refused at another check: \"$failed\", not \"$expected\""
        exit 1
        ;;
esac
