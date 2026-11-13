# Run by mac_update.rs with /bin/sh -c once the new RecompLauncher.app is in place. Every path is
# an argument, never part of the script's text.
pid=$1      # the version being replaced, still quitting
app=$2      # RecompLauncher.app, now the new version
old=$3      # the previous version, set aside
failed=$4   # where a new version that failed goes while the previous one is put back
program=$5  # the new version's program inside $app
open=$6     # /usr/bin/open (the tests pass a stand-in)

while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done

# Started directly, not with open, so its exit code is known here.
"$program" </dev/null >/dev/null 2>&1 &
child=$!

# Still running after 20 seconds: it started. The previous version isn't needed any more.
tries=0
while [ "$tries" -lt 100 ] && kill -0 "$child" 2>/dev/null; do
    sleep 0.2
    tries=$((tries + 1))
done
if kill -0 "$child" 2>/dev/null; then
    rm -rf "$old"
    exit 0
fi

# It quit that soon. With exit code 0 the player closed it, which isn't a failure. Anything else
# (a crash, or macOS refusing to run it) puts the previous version back and opens it.
wait "$child"
code=$?
if [ "$code" -eq 0 ]; then
    rm -rf "$old"
    exit 0
fi
[ -d "$old" ] || exit 1
rm -rf "$failed"
mv "$app" "$failed" || exit 1
if ! mv "$old" "$app"; then
    mv "$failed" "$app"
    exit 1
fi
rm -rf "$failed"
exec "$open" "$app"
