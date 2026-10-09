#!/usr/bin/env bash
# Fails if egui or eframe is in the dependency tree of a protocol crate, for any target.
# Arguments: packages to check, default oscq-core and oscq-discovery.
set -euo pipefail
packages=("$@")
if [ ${#packages[@]} -eq 0 ]; then
    packages=(oscq-core oscq-discovery)
fi
status=0
for pkg in "${packages[@]}"; do
    if ! tree=$(cargo tree --locked -p "$pkg" --target all -e normal,build --prefix none); then
        echo "FAIL: cargo tree failed for $pkg"
        exit 1
    fi
    if ! grep -q "^$pkg v" <<<"$tree"; then
        echo "FAIL: cargo tree output for $pkg does not list the package itself"
        exit 1
    fi
    if hits=$(grep -E '^(egui|eframe)([-_][a-z_-]+)? v' <<<"$tree" | sort -u) && [ -n "$hits" ]; then
        echo "FAIL: $pkg depends on UI crates:"
        echo "$hits"
        status=1
    else
        echo "ok: $pkg has no egui or eframe dependency"
    fi
done
exit $status
