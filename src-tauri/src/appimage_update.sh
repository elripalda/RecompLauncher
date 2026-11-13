# Run by /bin/sh -c with paths passed as arguments, never interpolated into shell source.
parent=$1
target=$2
work=$3
error=$4
reopen=$5
shift 5

report() { printf '%s\n' "$1" > "$error"; }
cleanup() { rm -rf -- "$work"; }

# Starting while the old launcher owns its D-Bus name would hand the new window back to it.
tries=0
while kill -0 "$parent" 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -ge 60 ]; then
        report "RecompLauncher didn't close, so its AppImage update was left unapplied. Try Update again."
        cleanup
        exit 1
    fi
    sleep 1
done

if ! mv -f -- "$work/next.AppImage" "$target"; then
    report "Couldn't replace the AppImage. The installed launcher hasn't changed. Check that its folder is writable, then try Update again."
    cleanup
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
            report "The new AppImage couldn't start. The previous version has been restored. Try Update again later."
            "$target" "$@" &
        else
            report "The new AppImage couldn't start. Restore previous.AppImage from $work to $target."
            exit 1
        fi
    fi
fi
cleanup
