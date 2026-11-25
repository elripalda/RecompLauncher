# Building RecompLauncher in a container

`build-in-container.sh` builds the Linux bundles in a throwaway container: the checkout is mounted
read-only, everything the build writes lives in a volume of its own, and the pass that produces the
artifacts runs with the network off.

The Flatpak build already runs inside a sandbox with a fixed list of what it may reach. This is for
the builds that aren't Flatpak — a `.deb`, an AppImage, or your own checkout — where the build
itself is the thing with access to everything you have.

## The isolated build

```sh
cd RecompLauncher
sandbox/build-in-container.sh              # fetch (network), then build offline -> sandbox/out/artifacts
```

What happens:

1. `fetch` — a container with the network on runs `npm ci`, `cargo fetch --locked` and one full
   build, so every download the build ever wants (npm packages, crates, Tauri's own AppImage
   tooling) lands in the container's private volume `rl-build-work`.
2. `offline` — the same build again with `--network none`. **This is the pass whose artifacts are
   kept.** A build that quietly phoned home could not have done it here.
3. The checkout is compared with `git status --porcelain` before and after, and the script says
   whether it changed. It cannot: it is mounted `:ro`.

The container itself:

* the checkout at `/src-ro`, read-only;
* a named volume at `/work` (source copy, cargo cache, target directory) that is not a host path;
* `sandbox/out` at `/out`, the only way anything leaves;
* runs as you, not as container root, so it reads your checkout with your own permissions and
  writes artifacts you already own — and it needs no capability for either;
* root filesystem read-only, `--cap-drop ALL`, `no-new-privileges`, `--pids-limit 8192`;
* no docker socket, no home directory, no SSH keys, no browser profile.

Stages can be run on their own: `--stage offline`, `--stage tests` (the project's own Node and
Rust test suites), `--rebuild-image`. `--no-trace` turns off the `strace` log of `connect()` calls
that the offline stage writes to `sandbox/out/connect-offline.log`.

### What is pinned, and why

Two builds of the same commit should be the same build, so everything movable in the image is
named exactly and moving it is a deliberate edit:

| What | Pinned as |
|---|---|
| the base image | `ubuntu:24.04@sha256:9c7f200c…`, a digest rather than a tag |
| the Rust toolchain | `ARG RUST_VERSION=1.100.0`, an exact release (the version used is in `build-info.txt`) |
| Node | `22.23.5` with nodejs.org's own SHA-256 |
| the app's dependencies | `Cargo.lock` and `package-lock.json`, both fetched with `--locked` |
| the AppImage type-2 runtime | the `20251108` release of `type2-runtime`, with that file's SHA-256 |

The AppImage runtime is the one thing the build needs but cannot download offline: `appimagetool`
fetches it while packaging, and Tauri passes ours through as `LDAI_RUNTIME_FILE`. A tagged release,
not the moving `continuous` one, so the pinned hash keeps matching; the fetch stage downloads it
once and the offline pass verifies it again.

Still floating: `apt-get install` in the image fetches whatever the package archive serves that
day. Pinning that as well means pointing the image at a dated snapshot of the archive
(`snapshot.ubuntu.com`), which is worth doing when a bit-exact rebuild is the goal rather than a
build that cannot surprise you.

### What it leaves in `sandbox/out/`

| File | |
|---|---|
| `artifacts/*.deb`, `artifacts/*.AppImage` | the two Linux bundles |
| `artifacts/recomplauncher` | the plain binary |
| `artifacts/SHA256SUMS` | checksums and sizes of everything above |
| `artifacts/build-info.txt` | the exact rustc, Node and CMake the image carried |
| `artifacts/Cargo.lock`, `artifacts/package-lock.json` | the inputs the build actually used |
| `container-build.log` | the whole run, ending with whether the checkout changed |
| `connect-offline.log` | every `connect()` the offline pass made |

## What this is not

* **Not a hermetic build.** The first pass has the network on: npm, crates.io and Tauri's own
  packaging tools have to be fetched from somewhere. Pinning what comes back, and running the
  artifact-producing pass without a network, is what this does instead.
* **Not a different build.** It runs the same `npm ci`, `npm run build` and `cargo build` the
  instructions above use, on the same checkout, and produces the same bundles.
* **Not a replacement for the Flatpak.** `packaging/flatpak` builds and runs the launcher inside
  Flatpak's sandbox, which covers players; this covers a build machine.

Nothing here changes the default way of building the launcher.
