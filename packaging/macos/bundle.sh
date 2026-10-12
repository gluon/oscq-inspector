#!/usr/bin/env bash
# Builds "Void OSCQ.app" from the aarch64 and x86_64 release builds, joined with lipo into one
# universal binary, ad hoc signed. Run on macOS from the workspace root, after:
#   cargo build --release --locked -p void-oscq --target aarch64-apple-darwin
#   cargo build --release --locked -p void-oscq --target x86_64-apple-darwin
# Arguments: version, output directory. Prints the path of the bundle.
set -euo pipefail
version="$1"
out="$2"
app="$out/Void OSCQ.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

lipo -create -output "$app/Contents/MacOS/void-oscq" \
    target/aarch64-apple-darwin/release/void-oscq \
    target/x86_64-apple-darwin/release/void-oscq
archs=$(lipo -archs "$app/Contents/MacOS/void-oscq")
if [[ "$archs" != *arm64* || "$archs" != *x86_64* ]]; then
    echo "FAIL: the binary is not universal, it holds: $archs"
    exit 1
fi

sed "s/__VERSION__/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
if grep -q "__VERSION__" "$app/Contents/Info.plist"; then
    echo "FAIL: version placeholder left in Info.plist"
    exit 1
fi

# Ad hoc signature, no identity: Apple Silicon runs no unsigned code, and a bundle whose seal does
# not match its content is reported as damaged. With it, macOS reports an unidentified developer,
# which the user can allow. A Developer ID signature replaces it in the disabled signing step.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict --verbose=2 "$app"
echo "$app"
