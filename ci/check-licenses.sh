#!/usr/bin/env bash
# Checks the license texts.
# Usage: check-licenses.sh            the font license texts in licenses/fonts are those of the
#                                     fonts in the binary, all of them and nothing else: the
#                                     fonts the epaint_default_fonts crate of Cargo.lock ships,
#                                     and the fonts this repository bundles itself
#        check-licenses.sh DIRECTORY  DIRECTORY holds the texts packaging/licenses.sh copies,
#                                     identical to the repository's
# Run from the workspace root.
set -euo pipefail

# The fonts this repository compiles into the binary itself, each with its license text. A font
# added here without its text, or a text left here without its font, fails below.
BUNDLED_DIR="crates/oscq-inspector/assets/fonts"

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

# The texts accounted for, so that a stale one left in licenses/fonts is caught at the end.
accounted=""

# 1. The fonts epaint ships: the text in licenses/fonts is a copy of the crate's own, byte for
#    byte, so an upgrade that changes a license is noticed.
epaint=0
for f in "$fonts"/*.txt; do
    if ! cmp -s "$f" "licenses/fonts/$(basename "$f")"; then
        echo "FAIL: licenses/fonts/$(basename "$f") is missing or differs from $f" >&2
        exit 1
    fi
    accounted="$accounted $(basename "$f")"
    epaint=$((epaint + 1))
done
# epaint_default_fonts ships four fonts with four license texts: none found means no check.
if [ "$epaint" -eq 0 ]; then
    echo "FAIL: no font license text in $fonts, the check cannot evaluate" >&2
    exit 1
fi

# 2. The fonts this repository bundles: there is no upstream copy in the build to compare with,
#    so the link checked is the one that matters. Each font file names its own family and its own
#    copyright in its `name` table, and the license text beside it must be the SIL Open Font
#    License and must carry that copyright line word for word. A text for another font, a
#    truncated text or a license of another kind fails.
if [ ! -d "$BUNDLED_DIR" ]; then
    echo "FAIL: no $BUNDLED_DIR, the check cannot evaluate" >&2
    exit 1
fi
bundled=0
for font in "$BUNDLED_DIR"/*.ttf; do
    if [ ! -f "$font" ]; then
        echo "FAIL: no font file in $BUNDLED_DIR, the check cannot evaluate" >&2
        exit 1
    fi
    base=$(basename "$font" .ttf)
    # SpaceMono-Regular.ttf carries the text SpaceMono-OFL.txt.
    text="licenses/fonts/${base%-*}-OFL.txt"
    if [ ! -s "$text" ]; then
        echo "FAIL: $text is missing or empty, the license of $font" >&2
        exit 1
    fi
    # The family and the copyright the font itself declares, from its `name` table.
    read -r family copyright <<EOF
$(python3 -I - "$font" <<'PY'
import struct, sys
b = open(sys.argv[1], "rb").read()
count = struct.unpack(">H", b[4:6])[0]
tables = {}
for i in range(count):
    off = 12 + 16 * i
    tag = b[off : off + 4].decode("latin1")
    start, length = struct.unpack(">II", b[off + 8 : off + 16])
    tables[tag] = (start, length)
start, _ = tables["name"]
records, strings = struct.unpack(">HH", b[start + 2 : start + 6])
found = {}
for i in range(records):
    rec = start + 6 + 12 * i
    pid, _, _, nid, ln, oo = struct.unpack(">HHHHHH", b[rec : rec + 12])
    if nid not in (0, 1):
        continue
    raw = b[start + strings + oo : start + strings + oo + ln]
    try:
        value = raw.decode("utf-16-be") if pid == 3 else raw.decode("latin1")
    except UnicodeDecodeError:
        continue
    found.setdefault(nid, value.strip())
# One line: the family with its spaces squeezed out, then the copyright.
print(found.get(1, "").replace(" ", ""), found.get(0, ""))
PY
)
EOF
    if [ -z "$family" ] || [ -z "$copyright" ]; then
        echo "FAIL: $font declares no family or no copyright, the check cannot evaluate" >&2
        exit 1
    fi
    if [ "$family" != "${base%-*}" ]; then
        echo "FAIL: $font is the family $family, not ${base%-*} as its name says" >&2
        exit 1
    fi
    if ! grep -qF "SIL OPEN FONT LICENSE Version 1.1" "$text"; then
        echo "FAIL: $text is not the SIL Open Font License version 1.1" >&2
        exit 1
    fi
    if ! grep -qF "$copyright" "$text"; then
        echo "FAIL: $text does not carry the copyright $font declares: $copyright" >&2
        exit 1
    fi
    accounted="$accounted $(basename "$text")"
    bundled=$((bundled + 1))
done
if [ "$bundled" -eq 0 ]; then
    echo "FAIL: no bundled font checked, the check cannot evaluate" >&2
    exit 1
fi

# 3. Nothing stale: every text in licenses/fonts belongs to a font in the binary.
for f in licenses/fonts/*.txt; do
    case " $accounted " in
    *" $(basename "$f") "*) ;;
    *)
        echo "FAIL: $f is the license of no font in the binary" >&2
        exit 1
        ;;
    esac
done

echo "ok: $epaint font license texts of $fonts, and $bundled of $BUNDLED_DIR, are in licenses/fonts"
