#!/usr/bin/env bash
#
# Starts the launcher from this build inside the sandbox, checks that it really came up (process
# alive, GPU device open, X11/Wayland window present), and stops it again.
#
# Run it from a normal terminal: bwrap cannot make a nested user namespace from inside another
# sandbox or container.
#
#   sandbox/smoke-launcher.sh [config]   (default: sandbox.json.example)
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER="$HERE/rl-sandbox"
CONFIG="${1:-$HERE/sandbox.json.example}"
LOG="$HERE/out/smoke.log"
SHOT="$HERE/out/smoke.png"
mkdir -p "$HERE/out"

status=0
fail() { echo "FAIL $*"; status=1; }
pass() { echo "PASS $*"; }

# Everything below only ever looks at descendants of this one bwrap, so a launcher someone has
# open outside the sandbox is never touched.
descendants() {
    local child
    for child in $(pgrep -P "$1" 2>/dev/null || true); do
        echo "$child"
        descendants "$child"
    done
}

launcher_of_sandbox() {
    local pid
    for pid in $(descendants "$1"); do
        [ "$(cat "/proc/$pid/comm" 2>/dev/null)" = "recomplauncher" ] && echo "$pid" && return 0
    done
    return 1
}

echo "=== starting the launcher under the sandbox ==="
"$RUNNER" --config "$CONFIG" >"$LOG" 2>&1 &
bwrap_pid=$!

# Give it time to start a web view, which is the slow part.
launcher_pid=""
for _ in $(seq 1 30); do
    launcher_pid="$(launcher_of_sandbox "$bwrap_pid" || true)"
    [ -n "$launcher_pid" ] && break
    kill -0 "$bwrap_pid" 2>/dev/null || break
    sleep 1
done

if ! kill -0 "$bwrap_pid" 2>/dev/null; then
    fail "the launcher exited within a few seconds; its output:"
    sed 's/^/    /' "$LOG" | tail -20
    exit 1
fi

if [ -z "$launcher_pid" ]; then
    fail "no recomplauncher process appeared"
else
    pass "launcher is running (inside pid $launcher_pid)"
    sleep 10
    if kill -0 "$launcher_pid" 2>/dev/null; then
        pass "still running after 10s"
    else
        fail "it died after starting"
    fi
fi

if [ -n "$launcher_pid" ] && [ -d "/proc/$launcher_pid" ] && command -v lsof >/dev/null; then
    if lsof -p "$launcher_pid" 2>/dev/null | grep -q '/dev/dri/'; then
        pass "it has the GPU open: $(lsof -p "$launcher_pid" 2>/dev/null | awk '/\/dev\/dri\// {print $NF}' | sort -u | head -n2 | tr '\n' ' ')"
    else
        fail "no /dev/dri device is open: it is rendering on the CPU"
    fi
    # A controller shows up here when one is plugged in: SDL keeps the node open for as long as it
    # is reading the pad. No pad, or a pad read through something else, is not a failure.
    opened="$(lsof -p "$launcher_pid" 2>/dev/null | grep -oE '/dev/(hidraw[0-9]+|input/[^ ]+)' | sort -u | tr '\n' ' ')"
    if [ -n "$opened" ]; then
        pass "input devices open: $opened"
    else
        echo "INFO no controller node is open (none plugged in, or it is read another way)"
    fi
fi

# A window is the point of a GUI: try both toolkits' ways of asking.
if command -v wmctrl >/dev/null && wmctrl -l 2>/dev/null | grep -qi 'recomp'; then
    pass "window present: $(wmctrl -l | grep -i recomp | head -n1)"
else
    echo "INFO no X11 window seen through wmctrl (a Wayland-native window will not show there)"
fi

if command -v gnome-screenshot >/dev/null; then
    if gnome-screenshot -f "$SHOT" 2>/dev/null && [ -s "$SHOT" ]; then
        pass "screenshot written to $SHOT"
    else
        echo "INFO screenshot not available in this session"
    fi
fi

echo
echo "=== launcher output so far ==="
sed 's/^/    /' "$LOG" | tail -15

echo
echo "=== stopping it ==="
# Ask the launcher itself first. bwrap forwards signals, but a GUI app closing its web view is not
# instant; if it has not gone in 15 seconds it is not going to.
if [ -n "$launcher_pid" ]; then
    kill "$launcher_pid" 2>/dev/null || true
    for _ in $(seq 1 15); do
        kill -0 "$launcher_pid" 2>/dev/null || break
        sleep 1
    done
    if kill -0 "$launcher_pid" 2>/dev/null; then
        echo "INFO the launcher was still there after SIGTERM and 15s; killing it"
        kill -9 "$launcher_pid" 2>/dev/null || true
    fi
fi

kill "$bwrap_pid" 2>/dev/null || true
sleep 2
if kill -0 "$bwrap_pid" 2>/dev/null; then
    kill -9 "$bwrap_pid" 2>/dev/null || true
fi

sleep 1
if [ -n "$launcher_pid" ] && kill -0 "$launcher_pid" 2>/dev/null; then
    fail "left behind: pid $launcher_pid is still running"
else
    pass "stopped, and nothing was left behind"
fi

exit "$status"
