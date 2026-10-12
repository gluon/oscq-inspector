#!/usr/bin/env bash
# Starts a built void-oscq with an empty configuration and fails unless it draws its first frame
# within 30 s. Needs a display. Argument: path to the binary.
set -euo pipefail
bin="$1"
home=$(mktemp -d)
log="$home/log.txt"
HOME="$home" XDG_CONFIG_HOME="$home" APPDATA="$home" NO_COLOR=1 \
    RUST_LOG=void_oscq::startup=info "$bin" >"$log" 2>&1 &
pid=$!
for _ in $(seq 1 300); do
    if grep -q "first frame" "$log"; then
        grep "first frame" "$log"
        kill "$pid"
        wait "$pid" 2>/dev/null || true
        echo "ok: $bin starts and draws its first frame"
        exit 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "FAIL: $bin exited before its first frame"
        cat "$log"
        exit 1
    fi
    sleep 0.1
done
kill "$pid" 2>/dev/null || true
echo "FAIL: no first frame from $bin within 30 s"
cat "$log"
exit 1
