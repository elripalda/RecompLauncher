# Flatpak / Flathub

App ID: `com.recomplauncher.RecompLauncher` (Flathub verifies it through recomplauncher.com).
The Tauri identifier stays `com.recomplauncher.app`, so other platforms keep their data folders.

## Build locally

```bash
git submodule add https://github.com/flathub/shared-modules packaging/flatpak/shared-modules   # once
flatpak install flathub org.gnome.Platform//51 org.gnome.Sdk//51 org.freedesktop.Sdk.Extension.rust-stable//26.08
curl -O https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
python3 flatpak-cargo-generator.py src-tauri/Cargo.lock -o packaging/flatpak/cargo-sources.json
cd packaging/flatpak
flatpak-builder --user --install --force-clean build com.recomplauncher.RecompLauncher.yml
flatpak run com.recomplauncher.RecompLauncher
```

(For a local build, swap the manifest's `git` source for `type: dir, path: ../..`.)

Lint before submitting: `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest com.recomplauncher.RecompLauncher.yml`
and `... appstream com.recomplauncher.RecompLauncher.metainfo.xml`.

## In the sandbox

- Updates come from Flathub: the launcher's own updater is off (`flatpak::sandboxed()`).
- Games ship as AppImages. They run inside the sandbox with `APPIMAGE_EXTRACT_AND_RUN=1` (there is
  no FUSE), unpacked under `~/.var/app/<id>/cache/appimage-run` instead of the sandbox's RAM-backed /tmp.
- A recomp that ships only a `.flatpak` file needs the PC's own flatpak, which the sandbox may not
  reach (Flathub flags `--talk-name=org.freedesktop.Flatpak`, so the manifest leaves it out). The
  launcher says so and shows the command that turns it on for the player:
  `flatpak override --user --talk-name=org.freedesktop.Flatpak com.recomplauncher.RecompLauncher`.
- Games, saves and settings live under `~/.var/app/com.recomplauncher.RecompLauncher/`.
  Folders the player picks go through the file portal.

## Not done yet

- Steam shortcuts (they'd need `flatpak run com.recomplauncher.RecompLauncher` as the program,
  and access to Steam's folder).
- A CI job that builds the Flatpak.
- The Flathub submission itself (a PR to flathub/flathub, with the release's commit hash in the manifest).
