#!/usr/bin/env bash
# Checks the license texts.
# Usage: check-licenses.sh            the font license texts in licenses/fonts are those of the
#                                     epaint_default_fonts crate in Cargo.lock, all of them
#        check-licenses.sh DIRECTORY  DIRECTORY holds the texts packaging/licenses.sh copies,
#                                     identical to the repository's
# Run from the workspace root.
set -euo pipefail
if [ $# -eq 1 ]; then
    dir="$1"
    n=0
    for f in LICENSE LICENSE-EXCEPTION licenses/fonts/*.txt; do
        packed="$dir/${f#licenses/}"
        if ! cmp -s "$f" "$packed"; then
            echo "FAIL: $packed is missing or differs from $f" >&2
            exit 1
        fi
        n=$((n + 1))
    done
    echo "ok: $n license texts in $dir"
    exit 0
fi
manifest=$(cargo metadata --format-version 1 --locked |
    python3 -c 'import json,sys; print(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"] == "epaint_default_fonts"))')
fonts="$(dirname "$manifest")/fonts"
n=0
for f in "$fonts"/*.txt; do
    if ! cmp -s "$f" "licenses/fonts/$(basename "$f")"; then
        echo "FAIL: licenses/fonts/$(basename "$f") is missing or differs from $f" >&2
        exit 1
    fi
    n=$((n + 1))
done
for f in licenses/fonts/*.txt; do
    if [ ! -f "$fonts/$(basename "$f")" ]; then
        echo "FAIL: $f is not a license text of $fonts" >&2
        exit 1
    fi
done
# epaint_default_fonts ships four fonts with four license texts: none found means no check.
if [ "$n" -eq 0 ]; then
    echo "FAIL: no font license text in $fonts, the check cannot evaluate" >&2
    exit 1
fi
echo "ok: the $n font license texts of $fonts are in licenses/fonts"
