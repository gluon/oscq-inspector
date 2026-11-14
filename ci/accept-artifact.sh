#!/usr/bin/env bash
# Checks the files of the GitHub Release, or of the draft, of one tag, with no one at the machine:
# what docs/acceptance.md section 0 asked a person to check that a script can. Run on macOS, from
# a checkout of the repository that holds the tag, with gh authenticated; a draft is listed only
# to a token that may write to the repository.
# Usage: accept-artifact.sh TAG             download the Release's files, check them and the
#                                           Release, then check the unzipped bundle as below
#        accept-artifact.sh --dir DIR TAG   check the bundle "OSCQuery Inspector.app" in DIR,
#                                           against the version and the license texts of TAG
# The two modes check a bundle with the same code. Each check prints what it proves and its
# result; the script exits at the first failure.
# For the workflow's refusals only, each a command run with the application's pid:
# ACCEPT_ARTIFACT_AFTER_START just after it is started, ACCEPT_ARTIFACT_BEFORE_QUIT just before
# the quit event is sent.
set -euo pipefail
dir=""
if [ "${1:-}" = "--dir" ]; then
    dir="${2:?usage: accept-artifact.sh --dir DIR TAG}"
    shift 2
fi
tag="${1:?usage: accept-artifact.sh TAG, or accept-artifact.sh --dir DIR TAG}"
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
pid=""
cleanup() {
    if [ -n "$pid" ]; then kill -KILL "$pid" 2>/dev/null || true; fi
    rm -rf "$work"
}
trap cleanup EXIT

# The Release itself: its files and its notes. Tag mode only.
release() {
    local mac="oscq-inspector-$version-macos-universal.zip"
    local win="oscq-inspector-$version-windows-x86_64.zip"
    local linux="oscq-inspector-$version-linux-x86_64.tar.gz"

    proves "The Release of $tag holds the three files, each named with its version, and nothing else"
    gh release download "$tag" --repo "$repo" --dir "$work/download" >/dev/null ||
        fail "no release or draft of $tag in $repo readable with this token"
    local listed expected
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

    proves "The Release's notes are the version's section of CHANGELOG.md at $tag"
    local body section
    body=$(gh release view "$tag" --repo "$repo" --json body --jq .body) || fail "the notes of $tag cannot be read"
    git show "$tag:CHANGELOG.md" >"$work/CHANGELOG.md"
    section=$(./ci/changelog-section.sh "$version" "$work/CHANGELOG.md") || fail "no section for $version in CHANGELOG.md at $tag"
    # The notes as the release workflow wrote them, trailing blank lines aside.
    [ "$(printf '%s' "$body" | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}')" = "$section" ] ||
        fail "the notes of $tag are not the section of $version in CHANGELOG.md at $tag"
    ok "$(printf '%s\n' "$section" | wc -l | tr -d ' ') lines, the section of $version"

    ditto -x -k "$work/download/$mac" "$work/unzipped" || fail "$mac does not unzip"
    dir="$work/unzipped"
}

# The bundle in $1: the same checks in both modes.
bundle() {
    local app="$1/OSCQuery Inspector.app"

    proves "The directory holds the application bundle"
    [ -d "$app" ] || fail "no OSCQuery Inspector.app in $1: $(ls "$1")"
    local plist="$app/Contents/Info.plist"
    [ -f "$plist" ] || fail "no Contents/Info.plist in the bundle"
    ok "$(basename "$app")"

    proves "The bundle carries the tag's version, as the Finder and Get Info read it"
    local key got
    for key in CFBundleShortVersionString CFBundleVersion; do
        got=$(/usr/libexec/PlistBuddy -c "Print :$key" "$plist" 2>/dev/null) || fail "no $key in Info.plist"
        [ "$got" = "$version" ] || fail "$key is \"$got\", not $version"
        echo "  $key $got"
    done
    ok "both version keys are $version"

    proves "The binary is universal: it runs natively on Apple Silicon and on Intel"
    local exe_name exe archs
    exe_name=$(/usr/libexec/PlistBuddy -c "Print :CFBundleExecutable" "$plist" 2>/dev/null) ||
        fail "no CFBundleExecutable in Info.plist"
    exe="$app/Contents/MacOS/$exe_name"
    [ -x "$exe" ] || fail "the executable $exe_name is missing or not executable"
    archs=$(lipo -archs "$exe")
    [[ " $archs " == *" arm64 "* && " $archs " == *" x86_64 "* ]] ||
        fail "the binary holds $archs, not arm64 and x86_64"
    ok "lipo -archs: $archs"

    proves "The bundle carries the license texts of the tag: the GPL, its additional permission, the four font licenses"
    local licenses="$app/Contents/Resources/licenses" n=0 f packed fonts
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
    local bundle_id home log started status
    bundle_id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$plist" 2>/dev/null) ||
        fail "no CFBundleIdentifier in Info.plist"
    home="$work/home"
    mkdir -p "$home"
    log="$work/start.log"
    # There before the start, so that a failure can always show it.
    : >"$log"
    HOME="$home" NO_COLOR=1 RUST_LOG=oscq_inspector::startup=info "$exe" >"$log" 2>&1 &
    pid=$!
    if [ -n "${ACCEPT_ARTIFACT_AFTER_START:-}" ]; then
        # Once the process is the application, not the shell that starts it.
        for _ in $(seq 1 50); do
            case "$(ps -o comm= -p "$pid" 2>/dev/null)" in
                *"$exe_name") break ;;
            esac
            sleep 0.1
        done
        echo "  after the start, as $(ps -o comm= -p "$pid" 2>/dev/null): $ACCEPT_ARTIFACT_AFTER_START $pid"
        $ACCEPT_ARTIFACT_AFTER_START "$pid"
    fi
    started=""
    for _ in $(seq 1 300); do
        if grep -q "first frame" "$log"; then
            started=yes
            break
        fi
        kill -0 "$pid" 2>/dev/null || { cat "$log" || true; fail "it exited before its first frame"; }
        sleep 0.1
    done
    [ -n "$started" ] || { cat "$log" || true; fail "no first frame within 30 s"; }
    echo "  $(grep "first frame" "$log" | tail -1)"
    if [ -n "${ACCEPT_ARTIFACT_BEFORE_QUIT:-}" ]; then
        echo "  before the quit event: $ACCEPT_ARTIFACT_BEFORE_QUIT $pid"
        $ACCEPT_ARTIFACT_BEFORE_QUIT "$pid"
    fi
    # The quit the application menu and the Dock send: an Apple event to the application, by the
    # identifier of its bundle, not a signal, which would end it without its own shutdown. Sent
    # without waiting for a reply, which an application that hangs would never give; and only to
    # a running application, which AppleScript would otherwise launch to deliver it.
    osascript -e "if application id \"$bundle_id\" is running then" \
        -e "ignoring application responses" \
        -e "tell application id \"$bundle_id\" to quit" \
        -e "end ignoring" \
        -e "end if" >/dev/null 2>"$work/osascript.log" ||
        { cat "$work/osascript.log" || true; fail "the quit event could not be sent"; }
    for _ in $(seq 1 150); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    if kill -0 "$pid" 2>/dev/null; then
        cat "$log" || true
        fail "still running 15 s after the quit event"
    fi
    status=0
    wait "$pid" || status=$?
    pid=""
    [ "$status" -eq 0 ] || { cat "$log" || true; fail "it quit with status $status, not 0"; }
    ok "first frame, then quit on the quit event, status 0"
}

if [ -z "$dir" ]; then
    release
fi
bundle "$dir"

echo
echo "ok: the bundle of $tag passes every check a script can make; Gatekeeper, the icon and the About box by the menu are for a person, docs/acceptance.md section 0"
