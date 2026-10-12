#!/usr/bin/env bash
# Prints the workspace version, the one version of every crate and of the macOS bundle.
# Usage: release-version.sh [TAG] [--plist INFO_PLIST]
# Fails unless every crate takes the workspace version and depends on the others at it, and
# unless both version keys of the Info.plist template are wired to it. With a tag, fails unless
# the tag is v followed by that version. With --plist, the Info.plist of a built bundle, fails
# unless both its version keys hold that version.
set -euo pipefail
tag=""
plist=""
while [ $# -gt 0 ]; do
    case "$1" in
        --plist) plist="${2:?--plist needs a file}"; shift 2 ;;
        *) tag="$1"; shift ;;
    esac
done

version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)
if [ -z "$version" ]; then
    echo "FAIL: no version in [workspace.package] of Cargo.toml" >&2
    exit 1
fi
internal=0
for manifest in crates/*/Cargo.toml; do
    if ! grep -q '^version.workspace = true$' "$manifest"; then
        echo "FAIL: $manifest does not take the workspace version" >&2
        exit 1
    fi
    # A dependency on another crate of the workspace names its version again: it must follow.
    # sed -E: the same extended syntax for GNU sed and the BSD sed of macOS.
    deps=$(sed -n -E 's/^((oscq-[a-z-]+)|(void-oscq)) = \{ version = "([^"]*)".*/\1 \4/p' "$manifest")
    internal=$((internal + $(printf '%s' "$deps" | grep -c . || true)))
    stale=$(printf '%s\n' "$deps" | awk -v v="$version" 'NF && $2 != v')
    if [ -n "$stale" ]; then
        echo "FAIL: $manifest depends on $stale, not $version" >&2
        exit 1
    fi
done
# void-oscq depends on oscq-core and oscq-discovery: finding none means the check did not run.
if [ "$internal" -eq 0 ]; then
    echo "FAIL: no dependency between the workspace crates found, the check cannot evaluate" >&2
    exit 1
fi

# The string under a key of a property list written one element per line.
plist_value() {
    awk -v key="<key>$2</key>" '
        found { if (match($0, /<string>[^<]*<\/string>/)) print substr($0, RSTART + 8, RLENGTH - 17); exit }
        index($0, key) { found = 1 }
    ' "$1"
}
for key in CFBundleShortVersionString CFBundleVersion; do
    got=$(plist_value packaging/macos/Info.plist "$key")
    if [ "$got" != "__VERSION__" ]; then
        echo "FAIL: $key of packaging/macos/Info.plist is \"$got\", not the __VERSION__ placeholder" >&2
        exit 1
    fi
    if [ -n "$plist" ]; then
        got=$(plist_value "$plist" "$key")
        if [ "$got" != "$version" ]; then
            echo "FAIL: $key of $plist is \"$got\", not the crate version $version" >&2
            exit 1
        fi
        echo "$key $got in $plist" >&2
    fi
done

if [ -n "$tag" ] && [ "$tag" != "v$version" ]; then
    echo "FAIL: tag $tag does not match the crate version $version" >&2
    exit 1
fi
echo "$version"
