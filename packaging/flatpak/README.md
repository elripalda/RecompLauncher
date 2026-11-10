# Flatpak / Flathub

App ID: `com.recomplauncher.RecompLauncher` (Flathub verifies it through recomplauncher.com).
The Tauri identifier stays `com.recomplauncher.app`, so other platforms keep their data folders.

## Build locally

```bash
git clone --depth 1 https://github.com/flathub/shared-modules packaging/flatpak/shared-modules   # once (git-ignored here)
flatpak install flathub org.gnome.Platform//51 org.gnome.Sdk//51 org.freedesktop.Sdk.Extension.rust-stable//26.08
curl -O https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
python3 flatpak-cargo-generator.py src-tauri/Cargo.lock -o packaging/flatpak/cargo-sources.json
cd packaging/flatpak
flatpak-builder --user --install --force-clean build com.recomplauncher.RecompLauncher.yml
flatpak run com.recomplauncher.RecompLauncher
```

(For a local build, swap the manifest's `git` source for `type: dir, path: ../..`, as the
Flatpak workflow does. In the flathub/flathub repo, shared-modules is a git submodule.)

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

- Steam and desktop shortcuts start `flatpak run com.recomplauncher.RecompLauncher`. The sandbox
  can't see Steam's process, so Steam counts as open while its command pipe has a reader
  (`~/.steam/steam.pipe`, or Flathub Steam's under `~/.var/app/com.valvesoftware.Steam`).

## Not done yet

- The Flathub submission itself (a PR to flathub/flathub). Before it: the manifest's `tag` must be
  a published release that has this packaging, with its `commit` filled in, and the metainfo needs
  that release in `<releases>`.
