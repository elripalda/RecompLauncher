#!/usr/bin/env bash
#
# Proves what the sandbox does, instead of trusting the config: runs a probe inside it and checks
# that the allowed paths are there and everything else is not.
#
# Needs bwrap to be able to make a namespace, so run it from a normal terminal (not from inside
# another sandbox or container). Everything it creates lives in a temporary folder outside the
# checkout (or an empty one named by RL_SANDBOX_TEST_ROOT) and is removed afterwards.
#
#   sandbox/test-sandbox.sh
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER="$HERE/rl-sandbox"
# Outside the checkout on purpose: if the test directories lived inside it, their parent
# directories would have to exist in the sandbox and the "not visible" checks would be meaningless.
WORKSPACE="$(cd "$HERE/.." && pwd)"
# RL_SANDBOX_TEST_ROOT picks the folder instead. It has to be empty or missing: everything in it
# is removed afterwards, so a folder with anything in it is refused rather than emptied.
if [ -n "${RL_SANDBOX_TEST_ROOT:-}" ]; then
    ROOT="$RL_SANDBOX_TEST_ROOT"
    if [ -e "$ROOT" ] && { [ ! -d "$ROOT" ] || [ -n "$(ls -A "$ROOT")" ]; }; then
        echo "RL_SANDBOX_TEST_ROOT=$ROOT exists and is not an empty folder; not touching it" >&2
        exit 1
    fi
    if [ -d "$ROOT" ]; then
        # Made by someone else: leave the folder, take out what this script put in it.
        trap 'find "$ROOT" -mindepth 1 -delete' EXIT
    else
        mkdir -p "$ROOT"
        trap 'rm -rf "$ROOT"' EXIT
    fi
else
    ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rl-sandbox-test.XXXXXX")"
    trap 'rm -rf "$ROOT"' EXIT
fi

mkdir -p "$ROOT/rw" "$ROOT/ro"
echo "host only" > "$ROOT/ro/read-only-file"
echo "host only" > "$ROOT/rw/before"

# A file inside the checkout: the sandbox must not see it unless the config says so.
readonly SECRET="$WORKSPACE/README.md"
[ -f "$SECRET" ] || { echo "no $SECRET to test with" >&2; exit 1; }

CONFIG="$ROOT/sandbox.json"
python3 - "$CONFIG" "$ROOT" "$WORKSPACE" <<'PY'
import json, sys
config, root, workspace = sys.argv[1:4]
json.dump({
    "version": 1,
    "network": False,
    "gpu": False, "input": False, "sound": False,
    "x11": False, "wayland": False, "session_bus": False,
    "auto_dirs": False,
    "read_only": [f"{root}/ro"],
    "read_write": [f"{root}/rw"],
    "shared_memory_mb": 64,
}, open(config, "w"), indent=2)
PY

NETWORK=0
cat > "$ROOT/ro/probe.py" <<PY
import json, os, socket, sys

root, workspace, secret = "$ROOT", "$WORKSPACE", "$SECRET"
home = os.path.expanduser("~")
failures = []

def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  ({detail})" if detail else ""))
    if not ok:
        failures.append(name)

# The allowed paths are there, with the content that was on the host.
check("read-only path visible", open(os.path.join(root, "ro/read-only-file")).read().strip() == "host only")
check("read-write path visible", open(os.path.join(root, "rw/before")).read().strip() == "host only")

# Read-write really is writable; read-only really is not.
try:
    open(os.path.join(root, "rw/created-by-sandbox"), "w").write("from inside")
    check("read-write path writable", True)
except OSError as e:
    check("read-write path writable", False, str(e))
try:
    open(os.path.join(root, "ro/should-not-exist"), "w").write("nope")
    check("read-only path refuses writes", False, "the write succeeded")
except OSError:
    check("read-only path refuses writes", True)

# Everything outside the config does not exist. Parent directories of an allowed path do have to
# exist for the mount to land somewhere, so they show up as empty directories; no file may come
# with them, and nothing else may be there.
leaked = []
for base, dirs, files in os.walk(home):
    leaked.extend(os.path.join(base, f) for f in files)
check("no file leaks in beside the allowed paths", not leaked, repr(leaked[:5]))
check("the checkout is not visible", not os.path.exists(workspace))
check("the file inside the checkout is not visible", not os.path.exists(secret))
check("~/.ssh is not visible", not os.path.exists(os.path.expanduser("~/.ssh")))
check("~/.config (someone else's settings) is not visible", not os.path.exists(os.path.expanduser("~/.config")))
check("~/Documents is not visible", not os.path.exists(os.path.expanduser("~/Documents")))
check("/var/log is not visible", not os.path.exists("/var/log"))
check("/etc/shadow is unreadable", not os.access("/etc/shadow", os.R_OK))

# The home directory itself is there, so a program that writes to ~ fails cleanly instead of
# tripping over a missing directory. It is a throwaway: whatever lands there is gone with the
# sandbox, which is checked from the host side below.
check("home exists", os.path.isdir(home))
try:
    open(os.path.join(home, "discarded-on-exit"), "w").write("from inside")
    check("home is a throwaway tmpfs", True)
except OSError as e:
    check("home is a throwaway tmpfs", False, str(e))

# No network when the config says so.
if "$NETWORK" == "0":
    try:
        s = socket.create_connection(("1.1.1.1", 53), timeout=3)
        s.close()
        check("network is blocked", False, "a connection to 1.1.1.1:53 succeeded")
    except OSError:
        check("network is blocked", True)

print()
if failures:
    print("failed: " + ", ".join(failures))
    sys.exit(1)
print("all checks passed")
PY

echo "=== running the probe inside the sandbox ==="
set +e
"$RUNNER" --config "$CONFIG" -- python3 "$ROOT/ro/probe.py"
status=$?
set -e

# The other half of the network question: with network: true the sandbox has to be able to resolve
# names and reach things. Without this check, handing the app the host's network while hiding the
# file /etc/resolv.conf points at looks like a working sandbox right up to the first request.
echo
echo "=== network: true: name resolution and a real request ==="
python3 - "$ROOT" "$CONFIG" >/dev/null <<'PY'
import json, sys
root, config = sys.argv[1:3]
cfg = json.load(open(config))
cfg["network"] = True
json.dump(cfg, open(f"{root}/ro/sandbox-net.json", "w"), indent=2)
PY

cat > "$ROOT/ro/probe-net.py" <<'PY'
import socket, sys
try:
    infos = socket.getaddrinfo("recomplauncher.com", 443, proto=socket.IPPROTO_TCP)
except OSError as e:
    print(f"FAIL name resolution inside the sandbox: {e}")
    sys.exit(1)
print(f"PASS name resolution inside the sandbox: {infos[0][4][0]}")
try:
    sock = socket.create_connection(("recomplauncher.com", 443), timeout=10)
    sock.close()
except OSError as e:
    print(f"FAIL a connection to recomplauncher.com:443: {e}")
    sys.exit(1)
print("PASS a TCP connection to recomplauncher.com:443")
PY

if python3 -c "import socket; socket.create_connection(('recomplauncher.com',443), timeout=5)" 2>/dev/null; then
    "$RUNNER" --config "$ROOT/ro/sandbox-net.json" -- python3 "$ROOT/ro/probe-net.py" || status=1
else
    echo "INFO this machine has no network right now; skipping the network: true check"
fi

# Devices are the other half of "it is there when it is allowed, and it is not when it is not".
# This is the check that was missing when a controller did not show up: /dev/input was in the
# sandbox and /dev/hidraw, which is how SDL reads an Xbox pad, was not.
echo
echo "=== devices: input and hidraw ==="
python3 - "$ROOT" "$CONFIG" >/dev/null <<'PY'
import json, sys
root, config = sys.argv[1:3]
cfg = json.load(open(config))
cfg["input"] = True
cfg["hidraw"] = True
json.dump(cfg, open(f"{root}/ro/sandbox-dev.json", "w"), indent=2)
PY

cat > "$ROOT/ro/probe-dev.py" <<'PY'
import glob, os, sys

want_hidraw, want_event, want_input = (int(a) for a in sys.argv[1:4])
got_input = os.path.isdir("/dev/input")
got_hidraw = len(glob.glob("/dev/hidraw*"))
got_event = len(glob.glob("/dev/input/event*"))

problems = []
if want_input and not got_input:
    problems.append("/dev/input is missing")
if not want_input and got_input:
    problems.append("/dev/input is there while input is off")
if got_hidraw != want_hidraw:
    problems.append(f"/dev/hidraw*: {got_hidraw} here, {want_hidraw} on the host")
if got_event != want_event:
    problems.append(f"/dev/input/event*: {got_event} here, {want_event} on the host")

# Opening one is what actually matters: being listed is not the same as being usable.
if want_hidraw:
    node = sorted(glob.glob("/dev/hidraw*"))
    if node:
        try:
            os.close(os.open(node[0], os.O_RDWR))
        except OSError as e:
            problems.append(f"cannot open {node[0]}: {e}")

for p in problems:
    print(f"FAIL {p}")
sys.exit(1 if problems else 0)
PY

host_hidraw=$(ls /dev/hidraw* 2>/dev/null | wc -l)
host_event=$(ls /dev/input/event* 2>/dev/null | wc -l)
echo "host has $host_event event device(s) and $host_hidraw hidraw node(s)"
set +e
"$RUNNER" --config "$ROOT/ro/sandbox-dev.json" -- python3 "$ROOT/ro/probe-dev.py" "$host_hidraw" "$host_event" 1
if [ $? -eq 0 ]; then
    echo "PASS input and hidraw are all there when both are on"
else
    status=1
fi
"$RUNNER" --config "$CONFIG" -- python3 "$ROOT/ro/probe-dev.py" 0 0 0
if [ $? -eq 0 ]; then
    echo "PASS no input device is there when input is off"
else
    status=1
fi
set -e

# nested_sandbox: umu-run (Proton) and WebKit make a user namespace of their own, so with the
# switch on that has to work, and with it off it has to be refused.
echo
echo "=== nested_sandbox: a user namespace made inside the sandbox ==="
if command -v unshare >/dev/null && unshare --user true 2>/dev/null; then
    for nested in true false; do
        python3 - "$CONFIG" "$ROOT/ro/sandbox-nested-$nested.json" "$nested" >/dev/null <<'PY'
import json, sys
config, out, nested = sys.argv[1:4]
cfg = json.load(open(config))
cfg["nested_sandbox"] = nested == "true"
json.dump(cfg, open(out, "w"), indent=2)
PY
        set +e
        "$RUNNER" --config "$ROOT/ro/sandbox-nested-$nested.json" -- unshare --user true 2>/dev/null
        got=$?
        set -e
        if [ "$nested" = true ] && [ $got -eq 0 ]; then
            echo "PASS nested_sandbox: true lets a program make its own user namespace"
        elif [ "$nested" = false ] && [ $got -ne 0 ]; then
            echo "PASS nested_sandbox: false refuses a nested user namespace"
        elif [ "$nested" = false ] && ! bwrap --help 2>&1 | grep -q -- --disable-userns; then
            echo "INFO this bwrap has no --disable-userns; nested_sandbox: false cannot be enforced"
        else
            echo "FAIL nested_sandbox: $nested (unshare --user exited with $got)"; status=1
        fi
    done
else
    echo "INFO this machine cannot make a user namespace with unshare; skipping the nested check"
fi

echo
echo "=== host side ==="
if [ -f "$ROOT/rw/created-by-sandbox" ]; then
    echo "PASS the file written inside the sandbox is on the host: $(cat "$ROOT/rw/created-by-sandbox")"
else
    echo "FAIL the file written inside the sandbox did not reach the host"; status=1
fi
if [ "$(cat "$ROOT/ro/read-only-file")" = "host only" ]; then
    echo "PASS the read-only path was not modified"
else
    echo "FAIL the read-only path changed"; status=1
fi
if [ -e "$HOME/discarded-on-exit" ]; then
    echo "FAIL a file written to the sandbox's home reached the real home"; status=1
else
    echo "PASS the sandbox's home is a throwaway: nothing reached the real home"
fi

exit $status
