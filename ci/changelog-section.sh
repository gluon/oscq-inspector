#!/usr/bin/env bash
# Prints the CHANGELOG.md section of one version, without its heading, for the release notes.
# Fails when the section is missing or empty: a tag must not be released without its notes.
# Argument: the version, without the v.
set -euo pipefail
version="$1"
file="${2:-CHANGELOG.md}"
body=$(awk -v v="$version" '
    index($0, "## [" v "]") == 1 { found = 1; next }
    found && /^## \[/ { exit }
    found && /^\[[^]]+\]: / { exit }
    found { print }
' "$file" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')
if [ -z "$body" ]; then
    echo "FAIL: no section for $version in $file" >&2
    exit 1
fi
printf '%s\n' "$body"
