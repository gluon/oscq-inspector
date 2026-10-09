#!/usr/bin/env bash
# Prints the size of a release binary and how it compares with the PRD 10 budget, which applies
# to macOS aarch64: under 25 MB. A measurement, not a gate: PRD 10 defers the remedy, a switch
# from wgpu to glow, to M5.
# Argument: path to the binary.
set -euo pipefail
bin="$1"
if [ ! -f "$bin" ]; then
    echo "FAIL: no binary at $bin"
    exit 1
fi
bytes=$(wc -c <"$bin" | tr -d ' ')
host=$(rustc -vV | sed -n 's/^host: //p')
budget=$((25 * 1024 * 1024))
mb=$(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 1048576 }')
if [ "$host" = "aarch64-apple-darwin" ]; then
    if [ "$bytes" -lt "$budget" ]; then verdict="within"; else verdict="OVER"; fi
    echo "BUDGET release binary size $host: $bytes bytes, $mb MB, $verdict the 25 MB budget"
else
    echo "BUDGET release binary size $host: $bytes bytes, $mb MB (the 25 MB budget is for aarch64-apple-darwin)"
fi
