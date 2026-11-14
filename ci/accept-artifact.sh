#!/usr/bin/env bash
# Checks the files of the GitHub Release, or of the draft, of one tag, with no one at the machine:
# what docs/acceptance.md section 0 asked a person to check that a script can. Run on macOS, from
# a checkout of the repository that holds the tag, with gh authenticated; a draft is listed only
# to a token that may write to the repository.
# Usage: accept-artifact.sh TAG
# Each check prints what it proves and its result; the script exits at the first failure.
set -euo pipefail
tag="${1:?usage: accept-artifact.sh TAG}"
repo="${GITHUB_REPOSITORY:-gluon/oscq-inspector}"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}
proves() {
    echo
    echo "== $1"
}
ok() {
    echo "ok: $*"
}

proves "The tag names a version, v and the version"
case "$tag" in
    v[0-9]*) version="${tag#v}" ;;
    *) fail "$tag is not v followed by a version" ;;
esac
[ "$(uname -s)" = "Darwin" ] || fail "the macOS checks need macOS, this is $(uname -s)"
git rev-parse --verify --quiet "$tag^{commit}" >/dev/null ||
    fail "$tag is not in this checkout: fetch it, the license texts are compared with its own"
ok "version $version, tag at $(git rev-parse --short "$tag^{commit}")"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mac="oscq-inspector-$version-macos-universal.zip"
win="oscq-inspector-$version-windows-x86_64.zip"
linux="oscq-inspector-$version-linux-x86_64.tar.gz"

proves "The Release of $tag holds the three files, each named with its version, and nothing else"
gh release download "$tag" --repo "$repo" --dir "$work/download" >/dev/null ||
    fail "no release or draft of $tag in $repo readable with this token"
listed=$(cd "$work/download" && ls -1 | sort)
expected=$(printf '%s\n' "$linux" "$mac" "$win" | sort)
if [ "$listed" != "$expected" ]; then
    fail "the release holds:
$listed
not:
$expected"
fi
for f in "$mac" "$win" "$linux"; do
    [ -s "$work/download/$f" ] || fail "$f is empty"
    echo "  $f, $(wc -c <"$work/download/$f" | tr -d ' ') bytes"
done
ok "three files, named with $version"

proves "The macOS zip holds one application bundle"
ditto -x -k "$work/download/$mac" "$work/unzipped" || fail "$mac does not unzip"
app="$work/unzipped/OSCQuery Inspector.app"
[ -d "$app" ] || fail "no OSCQuery Inspector.app in $mac: $(ls "$work/unzipped")"
plist="$app/Contents/Info.plist"
[ -f "$plist" ] || fail "no Contents/Info.plist in the bundle"
ok "$(basename "$app")"

proves "The bundle carries the tag's version, as the Finder and Get Info read it"
for key in CFBundleShortVersionString CFBundleVersion; do
    got=$(/usr/libexec/PlistBuddy -c "Print :$key" "$plist" 2>/dev/null) || fail "no $key in Info.plist"
    [ "$got" = "$version" ] || fail "$key is \"$got\", not $version"
    echo "  $key $got"
done
ok "both version keys are $version"

proves "The binary is universal: it runs natively on Apple Silicon and on Intel"
exe_name=$(/usr/libexec/PlistBuddy -c "Print :CFBundleExecutable" "$plist")
exe="$app/Contents/MacOS/$exe_name"
[ -x "$exe" ] || fail "the executable $exe_name is missing or not executable"
archs=$(lipo -archs "$exe")
[[ " $archs " == *" arm64 "* && " $archs " == *" x86_64 "* ]] ||
    fail "the binary holds $archs, not arm64 and x86_64"
ok "lipo -archs: $archs"

proves "The bundle carries the license texts of the tag: the GPL, its additional permission, the four font licenses"
licenses="$app/Contents/Resources/licenses"
n=0
for f in LICENSE LICENSE-EXCEPTION $(git ls-tree --name-only "$tag" licenses/fonts/ | grep '\.txt$'); do
    packed="$licenses/${f#licenses/}"
    [ -f "$packed" ] || fail "${packed#"$app/"} is missing"
    cmp -s <(git show "$tag:$f") "$packed" || fail "${packed#"$app/"} differs from $f at $tag"
    echo "  ${packed#"$app/"}"
    n=$((n + 1))
done
fonts=$(find "$licenses/fonts" -name '*.txt' | wc -l | tr -d ' ')
[ "$fonts" -eq 4 ] || fail "$fonts font licenses in the bundle, not four"
[ "$n" -eq 6 ] || fail "$n license texts at $tag, not six: the check cannot evaluate"
ok "six texts, identical to those of $tag"

proves "The application starts, draws its first frame, and quits cleanly when asked to"
bundle_id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$plist")
home="$work/home"
mkdir -p "$home"
log="$work/start.log"
HOME="$home" NO_COLOR=1 RUST_LOG=oscq_inspector::startup=info "$exe" >"$log" 2>&1 &
pid=$!
started=""
for _ in $(seq 1 300); do
    if grep -q "first frame" "$log"; then
        started=yes
        break
    fi
    kill -0 "$pid" 2>/dev/null || { cat "$log"; fail "it exited before its first frame"; }
    sleep 0.1
done
if [ -z "$started" ]; then
    kill "$pid" 2>/dev/null || true
    cat "$log"
    fail "no first frame within 30 s"
fi
echo "  $(grep "first frame" "$log" | tail -1)"
# The quit the application menu and the Dock send: an Apple event to the application, by the
# identifier of its bundle. Not a signal, which would end it without its own shutdown.
osascript -e "tell application id \"$bundle_id\" to quit" >/dev/null 2>"$work/osascript.log" ||
    { kill "$pid" 2>/dev/null || true; cat "$work/osascript.log"; fail "the quit event could not be sent"; }
for _ in $(seq 1 150); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.1
done
if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    cat "$log"
    fail "still running 15 s after the quit event"
fi
status=0
wait "$pid" || status=$?
[ "$status" -eq 0 ] || { cat "$log"; fail "it quit with status $status, not 0"; }
ok "first frame, then quit on the quit event, status 0"

echo
echo "ok: the files of $tag pass every check a script can make; Gatekeeper, the icon and the About box by the menu are for a person, docs/acceptance.md section 0"
