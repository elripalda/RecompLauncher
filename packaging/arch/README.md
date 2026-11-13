# Arch Linux

The native x86_64 package is built from source on Arch Linux with makepkg. It uses
the system's GTK 3 and WebKitGTK 4.1, with SDL's controller support linked statically.
It is a downloadable package, not an AUR submission or a pacman repository.

## Build locally

On an up-to-date Arch Linux system, install the build tools and libraries:

```sh
sudo pacman -Syu --needed base-devel rust cmake pkgconf git webkit2gtk-4.1 libappindicator desktop-file-utils namcap
bash tools/build-arch-package.sh --noconfirm
```

Run the helper from a checkout as your normal user. It takes the version from
`src-tauri/tauri.conf.json`, snapshots the working files (including local edits and
untracked, non-ignored source files), and writes a source archive and a rendered
`PKGBUILD` with its SHA-256 checksum to `dist/arch/`. Dependencies in `Cargo.lock`
are downloaded during `prepare()`; compilation uses `--frozen` and the production
`tauri/custom-protocol` feature. Node.js is not needed to build this package.

The result is `dist/arch/recomplauncher-<pkgver>-1-x86_64.pkg.tar.zst`. For example,
launcher version `0.90.0-beta.42` becomes Arch version `0.90.0beta.42`: makepkg does
not allow hyphens in `pkgver`, and this spelling sorts before `0.90.0` in pacman.
To rebuild the same version after local edits, pass `--force` to the helper.

```sh
namcap dist/arch/PKGBUILD dist/arch/*.pkg.tar.zst
sudo pacman -U dist/arch/recomplauncher-0.90.0beta.42-1-x86_64.pkg.tar.zst
```

The package installs `/usr/bin/recomplauncher`, the application menu entry, the
`recomplauncher://` URL handler and icons. Player data uses the existing
`com.recomplauncher.app` data directory. Install a newer package with `pacman -U`;
the launcher's Update Now button opens the download page for native Linux installs.

## CI and releases

`.github/workflows/arch.yml` builds in an `archlinux:base-devel` container as an
unprivileged user. PR checks call it for every PR and push to main; manual runs
retain the `installers-arch-x86_64` artifact for three days.
The workflow also installs the package inside its disposable container and checks
that its shared libraries can be resolved.

Release builds call the same workflow for the release tag, using the `release-Arch`
artifact. The release draft waits for the Arch job and includes its `.pkg.tar.zst`
alongside the other platforms. This change does not publish anything to AUR.
