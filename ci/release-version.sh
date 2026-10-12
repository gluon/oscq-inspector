#!/usr/bin/env bash
# Prints the workspace version, the one version of every crate. With a tag as argument, fails
# unless the tag is v followed by that version, and unless every crate takes the workspace version.
set -euo pipefail
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)
if [ -z "$version" ]; then
    echo "FAIL: no version in [workspace.package] of Cargo.toml" >&2
    exit 1
fi
for manifest in crates/*/Cargo.toml; do
    if ! grep -q '^version.workspace = true$' "$manifest"; then
        echo "FAIL: $manifest does not take the workspace version" >&2
        exit 1
    fi
done
if [ $# -ge 1 ] && [ "$1" != "v$version" ]; then
    echo "FAIL: tag $1 does not match the crate version $version" >&2
    exit 1
fi
echo "$version"
