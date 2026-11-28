#!/usr/bin/env bash
#
# Builds RecompLauncher inside a throwaway container, with the host checkout mounted read-only.
#
# What this gives you over `npm run build` on the host:
#   * the build runs as a container with no access to your home directory, your SSH keys, your
#     browser profile or the docker socket: it sees the checkout (read-only), a private volume,
#     and an output directory;
#   * root filesystem read-only, all capabilities dropped, no-new-privileges, bounded PIDs;
#   * the artifact-producing pass runs with the network switched OFF (--network none), so a build
#     that quietly phoned home could not have done so;
#   * the checkout is compared before and after, to show the build did not modify it.
#
# Usage:
#   sandbox/build-in-container.sh                 # fetch (network), then offline build -> sandbox/out
#   sandbox/build-in-container.sh --stage offline # offline build only (uses earlier caches)
#   sandbox/build-in-container.sh --stage tests   # the project's own test suites
#   sandbox/build-in-container.sh --rebuild-image # rebuild the image first
#
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="rl-build:latest"
WORK_VOLUME="rl-build-work"
OUT="$REPO/sandbox/out"
STAGE="all"
TRACE=1
REBUILD_IMAGE=0

while [ $# -gt 0 ]; do
    case "$1" in
        --stage) STAGE="${2:?}"; shift 2 ;;
        --out) OUT="${2:?}"; shift 2 ;;
        --image) IMAGE="${2:?}"; shift 2 ;;
        --rebuild-image) REBUILD_IMAGE=1; shift ;;
        --no-trace) TRACE=0; shift ;;
        -h|--help) sed -n '2,22p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

case "$STAGE" in
    all|fetch|offline|tests) ;;
    *) echo "--stage must be all, fetch, offline or tests" >&2; exit 2 ;;
esac

command -v docker >/dev/null || { echo "docker is not installed" >&2; exit 1; }

# Docker's own client state (buildx activity, credentials) is kept beside the output instead of in
# ~/.docker, so the whole procedure works under a sandbox that only allows writes in the checkout.
mkdir -p "$REPO/sandbox/.docker"
export DOCKER_CONFIG="${DOCKER_CONFIG:-$REPO/sandbox/.docker}"
[ -f "$REPO/src-tauri/Cargo.toml" ] || { echo "$REPO is not a RecompLauncher checkout" >&2; exit 1; }

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

# The build runs as this user, not as container root, so what it writes to $OUT is already ours
# and the read-only checkout is readable without any capability. The volume has to belong to the
# same user; that is a one-off fixup, checked cheaply on every run.
prepare_volume() {
    local owner
    owner="$(docker run --rm -v "$WORK_VOLUME":/work --entrypoint stat "$IMAGE" -c '%u:%g' /work 2>/dev/null || echo none)"
    if [ "$owner" != "$(id -u):$(id -g)" ]; then
        echo "== taking ownership of the build volume ($WORK_VOLUME) as $(id -un) =="
        docker run --rm -v "$WORK_VOLUME":/work --entrypoint chown "$IMAGE" -R "$(id -u):$(id -g)" /work
    fi
}

if [ "$REBUILD_IMAGE" = 1 ] || ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "== building the isolated build image ($IMAGE) =="
    docker build --progress=plain -t "$IMAGE" -f "$REPO/sandbox/Dockerfile" "$REPO/sandbox/"
fi

prepare_volume

# What the checkout looked like before, to prove afterwards that the build couldn't change it.
before="$(git -C "$REPO" status --porcelain --untracked-files=no)"
head_before="$(git -C "$REPO" rev-parse HEAD 2>/dev/null || echo unknown)"
echo "checkout:    $REPO"
echo "commit:      $head_before"
echo "dirty files: ${before:-none}"
echo "output:      $OUT"
echo "workspace:   docker volume $WORK_VOLUME (container-local, not a host path)"
echo

run_stage() {
    local stage="$1" network="$2" trace="$3"
    local -a extra=()
    [ "$trace" = 1 ] && extra+=(--cap-add SYS_PTRACE -e RL_TRACE=1)

    echo "================ stage: $stage (network: $network) ================"
    docker run --rm \
        --user "$(id -u):$(id -g)" \
        --name "rl-build-$stage" \
        --read-only \
        --tmpfs /tmp:rw,nosuid,nodev,size=8g \
        --tmpfs /run:rw,nosuid,nodev,size=64m \
        --cap-drop ALL \
        --security-opt no-new-privileges \
        --pids-limit 8192 \
        -e HOME=/work/home \
        -e CARGO_HOME=/work/cargo \
        -e CARGO_TARGET_DIR=/work/target \
        "${extra[@]}" \
        -v "$REPO":/src-ro:ro \
        -v "$WORK_VOLUME":/work \
        -v "$OUT":/out \
        --network "$network" \
        "$IMAGE" "$stage"
}

case "$STAGE" in
    all)
        # First pass with the network: everything the build downloads (npm packages, crates,
        # Tauri's packaging tools) is cached into the volume here and nowhere else.
        run_stage fetch bridge 0
        # Second pass with no network at all: this is the pass whose artifacts are kept.
        run_stage offline none "$TRACE"
        ;;
    fetch)   run_stage fetch bridge 0 ;;
    offline) run_stage offline none "$TRACE" ;;
    tests)   run_stage tests none "$TRACE" ;;
esac

after="$(git -C "$REPO" status --porcelain --untracked-files=no)"
echo
echo "================ result ================"
if [ "$before" = "$after" ]; then
    echo "host checkout: unchanged (no tracked file was modified by the build)"
else
    echo "host checkout: CHANGED during the build:"
    diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") || true
fi
echo "artifacts:"
ls -l "$OUT/artifacts" 2>/dev/null || echo "  (none: this stage does not produce them)"
[ -f "$OUT/artifacts/SHA256SUMS" ] && cat "$OUT/artifacts/SHA256SUMS"
if [ -f "$OUT/connect-offline.log" ]; then
    echo
    echo "connect() syscalls seen in the offline stage (should be unix/local only):"
    { sort "$OUT/connect-offline.log" | uniq -c | sort -rn | head -20; } || true
fi
