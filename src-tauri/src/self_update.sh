#!/bin/sh
# Embedded in the installed Mac binary and passed to /bin/sh -c after Tauri shuts down.
# Every path is an argument, never interpolated into shell source.
pid=$1
target=$2
next=$3
swap=$4
data=$5
error=$6
shift 6
previous="$swap/previous.app"

report() {
    printf '%s\n' "$1" > "$error"
}
cleanup() {
    /bin/rm -rf "$swap" "$data"
}
start() {
    /usr/bin/open -n "$target" --args "$@"
}

tries=0
while kill -0 "$pid" 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -ge 60 ]; then
        report "RecompLauncher didn't close, so the Mac update was left unapplied. Try Update again."
        cleanup
        exit 1
    fi
    /bin/sleep 1
done

# Recheck immediately before touching the installed app.
if ! /usr/bin/codesign --verify --deep --strict "$next"; then
    report "The staged Mac update has an invalid signature. The installed app hasn't changed."
    cleanup
    start "$@"
    exit 1
fi

if ! /bin/mv "$target" "$previous"; then
    report "Couldn't replace RecompLauncher. Check that its folder is writable, then try Update again."
    cleanup
    start "$@"
    exit 1
fi

if /bin/mv "$next" "$target" && start "$@"; then
    cleanup
    exit 0
fi

report "The new RecompLauncher couldn't be installed or started. Restoring the previous version."
if [ -e "$target" ] && ! /bin/mv "$target" "$swap/failed.app"; then
    /bin/rm -rf "$data"
    report "Couldn't restore RecompLauncher. The previous app is kept at $previous."
    exit 1
fi
if /bin/mv "$previous" "$target"; then
    cleanup
    start "$@"
else
    /bin/rm -rf "$data"
    report "Couldn't restore RecompLauncher. The previous app is kept at $previous."
fi
exit 1
