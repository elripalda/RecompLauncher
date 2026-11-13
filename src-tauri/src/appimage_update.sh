# Run by /bin/sh -c with paths passed as arguments, never interpolated into shell source.
parent=$1
target=$2
work=$3
trail=$4
reopen=$5
shift 5

# The launcher's own trail (launcher-trail.log), in its format, so a problem report shows it.
# The player hears about a failed update from update-pending.json on the next start instead.
note() { printf '%s UTC  AppImage update: %s\n' "$(date -u +%H:%M:%S)" "$1" >> "$trail"; }
cleanup() { rm -rf -- "$work"; }

# Starting while the old launcher owns its D-Bus name would hand the new window back to it.
tries=0
while kill -0 "$parent" 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -ge 60 ]; then
        note "RecompLauncher didn't close within a minute, so the update was left unapplied"
        cleanup
        exit 1
    fi
    sleep 1
done

if ! mv -f -- "$work/next.AppImage" "$target"; then
    note "Couldn't replace $target (is its folder writable?); the installed launcher hasn't changed"
    cleanup
    # The player still gets a window back: the unchanged launcher, which says the update didn't go in.
    if [ "$reopen" = 1 ]; then
        "$target" "$@" &
    fi
    exit 1
fi

if [ "$reopen" != 1 ]; then
    cleanup
    exit 0
fi

"$target" "$@" &
child=$!
# Catch immediate runtime failures (including missing libraries or an unexecutable image).
# This is a launch check, not confirmation that the new UI rendered successfully.
sleep 2
if ! kill -0 "$child" 2>/dev/null; then
    if ! wait "$child"; then
        if mv -f -- "$work/previous.AppImage" "$target"; then
            note "The new AppImage couldn't start; the previous version was restored"
            "$target" "$@" &
        else
            note "The new AppImage couldn't start, and restoring failed: copy previous.AppImage from $work to $target"
            exit 1
        fi
    fi
fi
cleanup
