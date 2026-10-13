#!/usr/bin/env bash
# Packs the Linux release binary, its .desktop file, its icon and the license texts in a tarball.
# Run from the workspace root after cargo build --release --locked -p void-oscq.
# Arguments: version, output directory. Prints the path of the tarball.
set -euo pipefail
version="$1"
out="$2"
name="void-oscq-$version-linux-x86_64"
dir="$out/$name"
rm -rf "$dir"
mkdir -p "$dir"
cp target/release/void-oscq "$dir/"
cp packaging/linux/void-oscq.desktop "$dir/"
# The icon, 256 pixels, drawn by the application's own code.
icons=$(mktemp -d)
cargo run --release --locked -p void-oscq --example icons -- "$icons" >&2
cp "$icons/void-oscq.png" "$dir/"
rm -rf "$icons"
if ! command -v desktop-file-validate >/dev/null; then
    echo "FAIL: desktop-file-validate not installed, the .desktop file cannot be checked"
    exit 1
fi
desktop-file-validate "$dir/void-oscq.desktop" >&2
./packaging/licenses.sh "$dir/licenses"
# The newest glibc symbol the binary needs: the oldest glibc it runs on.
glibc=$(objdump -T "$dir/void-oscq" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)
echo "requires $glibc or newer" >&2
tar -C "$out" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz"
