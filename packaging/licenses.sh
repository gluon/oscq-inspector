#!/usr/bin/env bash
# Copies the license texts every release artifact carries into a directory: the GNU GPL, its
# additional permission, and the licenses of the fonts compiled into the binary. Run from the
# workspace root. Argument: the destination directory, created if needed.
set -euo pipefail
dest="$1"
mkdir -p "$dest/fonts"
cp LICENSE LICENSE-EXCEPTION "$dest/"
cp licenses/fonts/*.txt "$dest/fonts/"
