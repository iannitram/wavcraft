#!/usr/bin/env bash
# N1.7 (#1107): the same steps through a running app (Remote, over the control socket) and
# through Headless, then the two saved files compared sample for sample. Starts the app with a
# fresh home and no file, runs crates/automation/examples/headless_window_parity.rs against it,
# and stops the app. Needs a display: a Linux machine with one, or `xvfb-run -a bash <this>`.
#
# Exit 0 only when the example's last line is `same=1 frames=9000` and it exited 0.
set -u

cd "$(dirname "$0")/../.." || exit 1

if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "no display: run this on a machine with a display (or under xvfb-run)"
    exit 2
fi

work=$(mktemp -d) || exit 1
app_pid=""

cleanup() {
    if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then
        kill -TERM "$app_pid" 2>/dev/null
        for _ in $(seq 1 100); do
            kill -0 "$app_pid" 2>/dev/null || break
            sleep 0.1
        done
        kill -KILL "$app_pid" 2>/dev/null
        wait "$app_pid" 2>/dev/null
    fi
    rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

cargo build --release -p wavcraft || { echo "the app did not build"; exit 1; }
cargo build --release -p wavcraft-automation --example headless_window_parity || {
    echo "the example did not build"
    exit 1
}

app="${WAVCRAFT_EXE:-target/release/wavcraft}"
example="target/release/examples/headless_window_parity"

port=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1])') || exit 1
token="$work/control.token"
mkdir -p "$work/home" "$work/sessions" "$work/logs"

env HOME="$work/home" XDG_CONFIG_HOME="$work/home/.config" XDG_CACHE_HOME="$work/home/.cache" \
    WAVCRAFT_SESSION_DIR="$work/sessions" WAVCRAFT_LOG_DIR="$work/logs" WAVCRAFT_NO_AUDIO=1 \
    WAVCRAFT_CONTROL_PORT="$port" WAVCRAFT_CONTROL_TOKEN_FILE="$token" \
    "$app" >"$work/app.log" 2>&1 &
app_pid=$!

for _ in $(seq 1 300); do
    [ -s "$token" ] && break
    if ! kill -0 "$app_pid" 2>/dev/null; then
        echo "the app exited before it opened the control channel"
        tail -n 20 "$work/app.log"
        exit 1
    fi
    sleep 0.1
done
if [ ! -s "$token" ]; then
    echo "the app did not write its control token in 30 seconds"
    tail -n 20 "$work/app.log"
    exit 1
fi

rc=0
out=$(WAVCRAFT_CONTROL_PORT="$port" WAVCRAFT_CONTROL_TOKEN_FILE="$token" timeout 180 "$example" "$work") || rc=$?
last=$(printf '%s\n' "$out" | tail -n 1)

if [ "$rc" -eq 124 ]; then
    echo "the app did not answer in 180 seconds"
    tail -n 20 "$work/app.log"
    exit 1
fi
if [ "$rc" -eq 0 ] && [ "$last" = "same=1 frames=9000" ]; then
    echo "$last"
    exit 0
fi

[ -n "$last" ] && echo "$last"
tail -n 20 "$work/app.log"
exit 1
