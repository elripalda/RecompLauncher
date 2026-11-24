#!/bin/bash
# Runs inside the isolated build container (see Dockerfile and build-in-container.sh).
#
# What it may touch:
#   /src-ro   the host checkout, mounted READ-ONLY. The build never writes here.
#   /work     a container-local volume: the source copy, the cargo cache, the target dir.
#   /out      the only place artifacts leave through.
#
# Stages:
#   fetch    network on: install npm packages, fetch crates, build and bundle once so every
#            download the build needs (including Tauri's own AppImage tooling) is cached.
#   offline  network OFF: the same build again, from the caches, producing the artifacts.
#   tests    network off: the project's own test suites.
#   shell    a shell in /work/src, for looking around.
#
# Any extra arguments after the stage are passed to cargo (`--locked`, `--offline`, ...).

set -euo pipefail

stage="${1:-offline}"
shift || true

export HOME=/work/home
export CARGO_HOME=/work/cargo
export CARGO_TARGET_DIR=/work/target
export PATH=/opt/toolchain-bin:/usr/local/bin:/usr/bin:/bin
# AppImage tooling is itself an AppImage, and a container has no FUSE mount: this makes it
# unpack itself instead. The project's own CI sets the same variable.
export APPIMAGE_EXTRACT_AND_RUN=1
# Docker mounts a --tmpfs with noexec, and the AppImage tooling unpacks itself and then runs the
# unpacked AppRun. /work is a real volume: executable, and still container-local.
export TMPDIR=/work/tmp

# appimagetool fetches the AppImage type-2 runtime from GitHub while it packages, which would make
# an offline pass impossible. Fetch it once in the networked pass instead, from a release that is
# named by its tag rather than by the moving "continuous" one, with the hash of that file pinned
# as well: the same two lines every time, and a deliberate edit to move them.
APPIMAGE_RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64"
APPIMAGE_RUNTIME_SHA256="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"
APPIMAGE_RUNTIME=/work/tauri-runtime/runtime-x86_64

# The runtime appimagetool fetches while packaging, kept in the volume so the offline pass has it.
# The comparison decides whether it is the right file, not whether a file is there: a pin that
# moves has to replace what the volume already holds, or every run after a pin change would fail
# until somebody cleared the cache by hand.
appimage_runtime_is_pinned() {
    [ -f "$APPIMAGE_RUNTIME" ] \
        && echo "$APPIMAGE_RUNTIME_SHA256  $APPIMAGE_RUNTIME" | sha256sum -c - >/dev/null 2>&1
}

fetch_appimage_runtime() {
    echo "+ fetching the AppImage runtime: $APPIMAGE_RUNTIME_URL"
    mkdir -p "$(dirname "$APPIMAGE_RUNTIME")"
    rm -f "$APPIMAGE_RUNTIME"
    curl -fsSL -o "$APPIMAGE_RUNTIME.part" "$APPIMAGE_RUNTIME_URL"
    if ! echo "$APPIMAGE_RUNTIME_SHA256  $APPIMAGE_RUNTIME.part" | sha256sum -c -; then
        echo "!! the AppImage runtime at $APPIMAGE_RUNTIME_URL is not the file this pin names." >&2
        echo "!! It comes from a tagged release, so this should not happen: update the pin" >&2
        echo "!! in sandbox/entry.sh deliberately if the tag really changed." >&2
        rm -f "$APPIMAGE_RUNTIME.part"
        exit 1
    fi
    mv "$APPIMAGE_RUNTIME.part" "$APPIMAGE_RUNTIME"
}

ensure_appimage_runtime() {
    if ! appimage_runtime_is_pinned; then
        if [ "$stage" != "fetch" ]; then
            echo "!! the AppImage runtime in the volume is not the one this pin names:" >&2
            echo "!! $APPIMAGE_RUNTIME_SHA256" >&2
            echo "!! Run --stage fetch once, which fetches it, before building offline." >&2
            exit 1
        fi
        fetch_appimage_runtime
    fi
    export LDAI_RUNTIME_FILE="$APPIMAGE_RUNTIME"
}

mkdir -p "$HOME" "$CARGO_HOME" "$CARGO_TARGET_DIR" /work/src /work/tmp /out

echo "===== RecompLauncher isolated build ====="
echo "stage:   $stage"
echo "extra:   $*"
echo "info:"
sed 's/^/  /' /opt/build-info.txt
echo "========================================="

# A fresh copy of the checkout. `--delete` keeps a rerun from mixing in files that are no longer
# in the source. .git is left out: the build has no use for it and it is the biggest thing here.
if [ "$stage" != "shell" ]; then
    # --no-owner/--no-group: the container drops every capability, so it cannot chown files to the
    # host's uid, and there is no reason to want it to.
    rsync -a --no-owner --no-group --delete \
        --exclude .git \
        --exclude node_modules \
        --exclude target \
        --exclude 'sandbox/out' \
        --exclude 'sandbox/.docker' \
        /src-ro/ /work/src/
fi

cd /work/src

# linuxdeploy's AppImage plugin takes the runtime through this variable (it is what its
# LDAI_RUNTIME_FILE name is for); with it there is nothing left to download while bundling.
ensure_appimage_runtime

# strace truncates its -o file, so each traced command gets its own file and they are merged at
# the end. Needs SYS_PTRACE in the container, which the run script adds only when tracing.
RUN_SEQ=0
run() {
    echo "+ $*" >&2
    if [ "${RL_TRACE:-0}" = "1" ]; then
        RUN_SEQ=$((RUN_SEQ + 1))
        strace -f -qq -e trace=connect -o "/out/connect-${stage}-${RUN_SEQ}.log" -- "$@"
    else
        "$@"
    fi
}

merge_traces() {
    local parts=(/out/connect-"${stage}"-*.log)
    [ -e "${parts[0]}" ] || return 0
    cat "${parts[@]}" > "/out/connect-${stage}.log"
    rm -f "${parts[@]}"
}

# One bundle at a time, so a failure in one does not lose the other.
build_installers() {
    local extra=("$@")
    local rc=0
    for bundle in deb appimage; do
        echo "--- bundling $bundle ---"
        run ./node_modules/.bin/tauri build --bundles "$bundle" -- "${extra[@]}" || rc=$?
    done
    return "$rc"
}

collect() {
    local release="$CARGO_TARGET_DIR/release"
    mkdir -p /out/artifacts
    local found=0
    for pattern in "$release"/bundle/deb/*.deb "$release"/bundle/appimage/*.AppImage; do
        for f in $pattern; do
            [ -e "$f" ] || continue
            cp -a "$f" /out/artifacts/
            found=$((found + 1))
        done
    done
    # The plain binary too: it is what the bwrap runner starts when no bundle is wanted.
    if [ -x "$release/recomplauncher" ]; then
        cp -a "$release/recomplauncher" /out/artifacts/recomplauncher
        found=$((found + 1))
    fi
    if [ "$found" -eq 0 ]; then
        echo "!! nothing was collected: the build produced no artifacts" >&2
        return 1
    fi
    cp /opt/build-info.txt /out/artifacts/build-info.txt
    # The exact inputs, kept next to the output so the artifact can be traced back to them.
    cp src-tauri/Cargo.lock package-lock.json /out/artifacts/ 2>/dev/null || true
    (cd /out/artifacts && sha256sum -- * > SHA256SUMS)
    echo "--- artifacts ---"
    (cd /out/artifacts && ls -l && cat SHA256SUMS)
}

case "$stage" in
fetch)
    npm ci --no-audit --no-fund
    cargo fetch --locked --manifest-path src-tauri/Cargo.toml
    # Bundling downloads Tauri's packaging tools (linuxdeploy and friends) from GitHub. Doing it
    # here means the offline run below never needs the network.
    rc=0
    build_installers --locked || rc=$?
    collect || rc=${rc:-1}
    exit "$rc"
    ;;
offline)
    export CARGO_NET_OFFLINE=true
    run npm ci --offline --no-audit --no-fund
    run cargo build --release --locked --offline --manifest-path src-tauri/Cargo.toml
    rc=0
    build_installers --locked --offline || rc=$?
    merge_traces
    collect || rc=${rc:-1}
    exit "$rc"
    ;;
tests)
    export CARGO_NET_OFFLINE=true
    npm ci --offline --no-audit --no-fund
    node --test tests/*.test.cjs
    node --check ui/app.js
    cargo test --locked --offline --manifest-path src-tauri/Cargo.toml
    ;;
shell)
    exec bash
    ;;
*)
    echo "unknown stage: $stage" >&2
    exit 2
    ;;
esac
