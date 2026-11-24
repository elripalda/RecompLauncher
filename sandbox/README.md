# Building and running RecompLauncher with a boundary around it

The Flatpak build already runs the launcher in a sandbox with a fixed list of what it may reach;
this is the same idea for the builds that aren't Flatpak — a `.deb`, an AppImage, or your own
checkout — with the list in a JSON file you can read and change. Nothing here replaces the Flatpak;
if you use it, you already have a boundary at least as tight as this one.

Two things live here:

| Problem | What does it | Guarantee |
|---|---|---|
| The build runs on your machine and can do anything it likes there | `build-in-container.sh` + `Dockerfile` | The build sees the checkout read-only, a private volume and an output directory. The pass that produces the artifacts runs with the network **off**. |
| The launcher, once built, downloads other people's programs and starts them | `rl-sandbox` + `sandbox.json` | Outside the paths named in the config, the launcher sees an empty filesystem. |

Neither half is a fix for the launcher: both are a boundary around it, for a machine where you want
to know what a build and a launcher actually touch. What it does not cover is at the end of this
file.

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
| `artifacts/recomplauncher` | the plain binary, the same one the runner starts |
| `artifacts/SHA256SUMS` | checksums and sizes of everything above |
| `artifacts/build-info.txt` | the exact rustc, Node and CMake the image carried |
| `artifacts/Cargo.lock`, `artifacts/package-lock.json` | the inputs the build actually used |
| `container-build.log` | the whole run, ending with whether the checkout changed |
| `connect-offline.log` | every `connect()` the offline pass made |

## Running it safely

```sh
cp sandbox/sandbox.json.example ~/.config/recomplauncher/sandbox.json
$EDITOR ~/.config/recomplauncher/sandbox.json     # list the folders it may touch
sandbox/rl-sandbox --list                         # check what it will expose
sandbox/rl-sandbox                                # run the launcher
```

* `read_write` — folders the launcher may change (where games go).
* `read_only` — folders it may read (game libraries, disc images, `/run/media`).
* Nothing else exists inside. `~/.ssh`, other projects, `~/Documents`, `/var`: not there.
* The launcher's own data folder (`~/.local/share/com.recomplauncher.app`, what `app_data_dir()`
  resolves to) is added automatically (`auto_dirs`), and a writable path is created if it is
  missing. The runner's config is deliberately not among them: a sandboxed launcher should not be
  able to rewrite the policy it runs under.
* `network`, `gpu`, `input`, `hidraw`, `sound`, `x11`, `wayland`, `session_bus` each switch a piece
  on or off. `input` is the switch for controllers: with it off neither the event devices nor the
  raw HID nodes exist, and `hidraw: false` keeps only the event devices — an Xbox pad is read
  through `/dev/hidraw`, so turning that off is how a controller goes missing. With
  `network: false` the sandbox has no network at all.
* Anything after `--` runs in the same sandbox instead of the launcher, which is how you look
  around inside it: `sandbox/rl-sandbox -- /bin/sh`.

Two things to know about actually starting it:

* **Run it from a normal terminal.** Inside another sandbox (a container, an agent's shell) there
  is no room for a nested user namespace and bwrap will refuse with `setting up uid map: Permission
  denied`. On Ubuntu 24.04 the same message can come from AppArmor restricting unprivileged user
  namespaces; the runner prints what to do about it.
* **The GUI needs the display and the GPU.** `--list` will show whether `/dev/dri` and the X11 or
  Wayland socket are in; if `gpu` is on and the launcher reports `llvmpipe`, the GPU never made it
  through.

If the launcher says it cannot reach recomplauncher.com while `network: true`: sharing the host's
network is not the same as being able to resolve names. On Ubuntu `/etc/resolv.conf` is a symlink
to `/run/systemd/resolve/stub-resolv.conf`, and `/run` inside the sandbox is an empty directory —
so the file dangles and every lookup fails while the network itself works fine. The runner binds
the symlink's target as well (it shows up in `--list`); the snippet below is the part that is easy
to miss when writing a bwrap command by hand:

```
--dir /run/systemd/resolve \
--ro-bind /run/systemd/resolve/stub-resolv.conf /run/systemd/resolve/stub-resolv.conf
```

`test-sandbox.sh` covers this in both directions: with `network: false` a connection must fail,
and with `network: true` a name must resolve and a real TCP connection must go through.

### Proving it, rather than trusting it

```sh
sandbox/test-sandbox.sh       # what the sandbox hides, checked both ways
sandbox/smoke-launcher.sh     # the built launcher really starts in it, on the GPU
```

`test-sandbox.sh` runs a probe inside the sandbox and checks both sides: the allowed paths are
visible and behave as configured (writable stays writable, read-only refuses writes), and
everything else — the checkout, `~/.ssh`, `~/.config`, `~/Documents`, `/var/log`, `/etc/shadow` —
is not there. It also checks that a file written into the sandbox's home never reaches the real
home, that a file written to an allowed folder *does* reach it, and that `network: false` really
blocks the network while `network: true` resolves a name and connects. Devices are checked the same
two ways, which is how a missing controller gets caught: with `input` on, every event device and
raw HID node the host has must be there and openable; with it off, none of them may be. All of it
runs against a throwaway directory outside the checkout, removed afterwards.

`smoke-launcher.sh` starts the launcher from this build through the sandbox, waits, and checks
that it is still running, has `/dev/dri` open (so it is not falling back to `llvmpipe`), and leaves
a screenshot in `sandbox/out/smoke.png`; then it stops it and checks nothing was left behind. It
only ever touches processes it started itself.

Both need a normal shell rather than another sandbox (bwrap cannot build a nested user namespace);
`smoke-launcher.sh` also needs the session's display and GPU.

## What this is not

* **Not the first sandbox the project has.** `packaging/flatpak` is the maintained one, and it
  covers players who install from Flathub, including the games it starts. This is for the `.deb`,
  the AppImage and a checkout, where nothing would otherwise be in the way.
* **Not a fix for the launcher.** Inside the folders you allow it does everything it is designed to
  do, including starting a program named by a `recomplauncher.json` from someone else's release.
  The writable list is what limits the consequences, so keep it to a library folder rather than a
  home directory, and read `--list` before trusting it.
* **Not a container.** There is no second root filesystem and no image for the launcher itself: the
  machine's own `/usr`, `/etc` and kernel are used, mounted read-only.
* **Not hidden from the desktop.** With `x11`, `wayland` and `session_bus` on, windows,
  notifications and the tray behave as usual; the boundary is about files and devices.
* **Not a network filter.** `network: false` is all or nothing, and `network: true` is the host's
  own network with no per-host rules.
