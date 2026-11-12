#!/bin/sh
# Embedded in the installed Mac binary and passed to /bin/sh -c after Tauri shuts down.
# Every path is an argument, never interpolated into shell source.
pid=$1
target=$2
next=$3
swap=$4
data=$5
error=$6
program=$7
token=$8
shift 8
previous="$swap/previous.app"
startup_tries=60

report() {
    printf '%s\n' "$1" > "$error"
}
cleanup() {
    /bin/rm -rf "$swap" "$data"
}
start() {
    /usr/bin/open -n "$target" --args "$@"
}

startup_acknowledged() {
    [ -f "$swap/startup.ready" ] || return 1
    read -r ready_pid ready_token < "$swap/startup.ready"
    [ "$ready_pid" = "$child" ] && [ "$ready_token" = "$token" ]
}

wait_for_startup() {
    tries=0
    while [ "$tries" -lt "$startup_tries" ]; do
        kill -0 "$child" 2>/dev/null || return 1
        if startup_acknowledged; then
            # Keep the backup through an immediate crash after the first rendered frame too.
            /bin/sleep 1
            kill -0 "$child" 2>/dev/null
            return $?
        fi
        tries=$((tries + 1))
        /bin/sleep 1
    done
    return 1
}

stop_replacement() {
    # Stop a hung replacement before restoring files or reopening the old single instance.
    : > "$swap/startup.cancelled"
    if ! kill -0 "$child" 2>/dev/null; then
        wait "$child" 2>/dev/null
        return 0
    fi
    if ! kill -TERM "$child" 2>/dev/null && kill -0 "$child" 2>/dev/null; then
        return 1
    fi
    tries=0
    while kill -0 "$child" 2>/dev/null && [ "$tries" -lt 5 ]; do
        /bin/sleep 1
        tries=$((tries + 1))
    done
    if ! kill -0 "$child" 2>/dev/null; then
        wait "$child" 2>/dev/null
        return 0
    fi
    if ! kill -KILL "$child" 2>/dev/null && kill -0 "$child" 2>/dev/null; then
        return 1
    fi
    wait "$child" 2>/dev/null
    return 0
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

if /bin/mv "$next" "$target"; then
    # Launch the installed executable directly so the helper owns its PID, including when it
    # hangs or dies before Rust/JavaScript can send the readiness acknowledgement.
    "$program" --update-startup "$swap" --update-token "$token" "$@" &
    child=$!
    if wait_for_startup; then
        cleanup
        exit 0
    fi
    if ! stop_replacement; then
        /bin/rm -rf "$data"
        report "The Mac update didn't confirm startup and couldn't be stopped. The previous app is kept at $previous."
        exit 1
    fi
fi

report "The new RecompLauncher didn't confirm successful startup. Restoring the previous version."
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
