# Your recomp in the RecompLauncher Flatpak

RecompLauncher is also a Flatpak (`com.recomplauncher.RecompLauncher`, on Flathub). Games it
starts run inside its sandbox, with the same permissions. Most recomps work there unchanged. This
page lists what's different from the AppImage and .deb builds of the launcher, and what to do so
your game behaves the same everywhere.

Packaging notes for the launcher itself are in
[packaging/flatpak/README.md](../packaging/flatpak/README.md).

## What your game can see

| Where | In the Flatpak |
|---|---|
| The game's own folder (`{install}`) | Real and writable. Your game can write next to its program, unlike inside a mounted AppImage. |
| `~/Games` and other drives (`/run/media`, `/media`, `/mnt`) | Real, read and write. The default install folder is `~/Games/Recomp Library`. |
| `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` | Point into `~/.var/app/com.recomplauncher.RecompLauncher/` (`data`, `config`, `cache`, `.local/state`). Kept between runs. |
| Anything else in the home folder | Not there. The sandbox's home is in memory: what your game writes to `~/.mygame` or a hard-coded `~/.local/share/...` is **lost when it closes**. |
| Files the player picked elsewhere | May arrive as `/run/user/<uid>/doc/<id>/...` paths (the file chooser portal). Treat them like any other path. |
| The PC's processes and programs | Not visible. Your game can't start programs installed on the PC. |

**Saves and settings:** use the XDG folders, or `SDL_GetPrefPath`, which follows them, or your
game's own folder. Don't build paths from `$HOME` by hand. A game that does keeps nothing between
runs in the Flatpak.

## AppImages are unpacked at install

There's no FUSE in the sandbox, so an AppImage can't mount itself. When the Flatpak launcher
installs or updates a release with one AppImage at its top (on its own, or inside a zip), it unpacks
it like an archive:

- The AppImage's contents (`AppRun`, `usr/`, the `.desktop` file, an embedded
  `recomplauncher.json`) become the game's folder, and the AppImage file is removed.
- The game starts from `AppRun`, with `APPDIR` set to the game's folder and `ARGV0` to `AppRun`,
  as the AppImage runtime would set them. **`APPIMAGE` is not set**, since there's no AppImage file.
- `"program": ["MyGame.AppImage"]` in your `recomplauncher.json` keeps working: when the AppImage
  isn't there and an `AppRun` is, the launcher starts the `AppRun`. The same goes for `prepare`.
- An embedded `recomplauncher.json` ends up next to `AppRun`, where the launcher reads it.

So in your AppImage:

- **Find your files from `APPDIR` or from your own program's path** (`/proc/self/exe`), never from
  `APPIMAGE` or a mount point.
- **Don't update yourself through `APPIMAGE`.** Updates come from your GitHub releases through the
  launcher.
- A release with two or more AppImages isn't unpacked at install. Each unpacks the first time it's
  played instead, which is slower, so ship one.

Games the player brought in themselves (Bring Your Recomps, Set Up By Hand) aren't installed by
the launcher. If one is an AppImage, it's unpacked the first time it's played, into the launcher's
cache, and reused after that.

## Libraries

Inside the Flatpak, "the system" is the GNOME 51 runtime (freedesktop SDK 26.08), not the player's
distro. A `.tar.gz` that relies on libraries from the PC gets the runtime's versions, and anything
the runtime lacks is missing. An AppImage brings its own libraries, which is why it's the safer
choice for the Flatpak. Either way, bundle what your game needs beyond the basics (glibc, Vulkan
and OpenGL loaders, X11/Wayland, PulseAudio/PipeWire).

Graphics drivers come from the runtime's GL extension. Controllers and other devices are available
(`--device=all`).

## Other differences

- **`.flatpak` releases** need the PC's own Flatpak, which the sandbox can't reach unless the player
  allows it. Players get a message with the command to allow it. Prefer an AppImage or a `.tar.gz`.
- **Telling where you run:** `FLATPAK_ID` is set to `com.recomplauncher.RecompLauncher` (and
  `/.flatpak-info` exists). The usual `RECOMPLAUNCHER`, `RECOMPLAUNCHER_VERSION`,
  `RECOMPLAUNCHER_MODE` and `RECOMPLAUNCHER_PAD` variables are set as on every build.
- **Steam:** "Add to Steam" works. Steam starts your game through `flatpak run
  com.recomplauncher.RecompLauncher --play <id>`, so it runs in the sandbox there too.

## Trying your game in the sandbox

Install the Flatpak launcher, then open a shell in the same sandbox your game runs in:

```bash
flatpak run --command=sh com.recomplauncher.RecompLauncher
```

From there, start your game from its install folder (`~/Games/Recomp Library/<game>/AppRun` for an
AppImage the launcher unpacked) and check that it starts, finds its files and keeps its saves after
it closes.
