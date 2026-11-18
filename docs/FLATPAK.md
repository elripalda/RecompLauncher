# Your recomp in the RecompLauncher Flatpak

RecompLauncher can also be built as a Flatpak (`com.recomplauncher.RecompLauncher`). It isn't on
Flathub yet: the packaging is ready for a submission. Games the Flatpak starts run inside its
sandbox, with the same permissions. Most recomps work there unchanged. This
page lists what's different from the AppImage and .deb builds of the launcher, and what to do so
your game behaves the same everywhere.

Packaging notes for the launcher itself are in
[packaging/flatpak/README.md](../packaging/flatpak/README.md).

## What your game can see

| Where | In the Flatpak |
|---|---|
| The game's own folder (`{install}`) | Real and writable. Your game can write next to its program, as on every Linux build (the launcher unpacks AppImages). |
| `~/Games` and other drives (`/run/media`, `/media`, `/mnt`) | Real, read and write. The default install folder is `~/Games/Recomp Library`. |
| `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` | Point into `~/.var/app/com.recomplauncher.RecompLauncher/` (`data`, `config`, `cache`, `.local/state`). Kept between runs. |
| Anything else in the home folder | Not there. The sandbox's home is in memory: what your game writes to `~/.mygame` or a hard-coded `~/.local/share/...` is **lost when it closes**. |
| Files the player picked elsewhere | May arrive as `/run/user/<uid>/doc/<id>/...` paths (the file chooser portal). Treat them like any other path. |
| The PC's processes and programs | Not visible. Your game can't start programs installed on the PC. |

**Saves and settings:** use the XDG folders, or `SDL_GetPrefPath`, which follows them, or your
game's own folder. Don't build paths from `$HOME` by hand. A game that does keeps nothing between
runs in the Flatpak.

## AppImages are unpacked at install

On Linux the launcher never runs an AppImage, in the Flatpak or out of it: every game plays from a
plain folder. Your AppImage is unpacked at install with the launcher's own SquashFS reader (it
needs no FUSE, and nothing of the image runs), and the game starts from its `AppRun`. What that
means for your AppImage is in [AppImages](DEVELOPERS.md#appimages) in the developer guide.

Inside the Flatpak this is also why it works at all: there's no FUSE in the sandbox, so an
AppImage couldn't mount itself there.

## Libraries

Inside the Flatpak, "the system" is the GNOME 51 runtime (freedesktop SDK 26.08), not the player's
distro. A tarball that relies on libraries from the PC gets the runtime's versions, and anything
the runtime lacks is missing. An AppImage brings its own libraries, which is why it's the safer
choice for the Flatpak. Either way, bundle what your game needs beyond the basics (glibc, Vulkan
and OpenGL loaders, X11/Wayland, PulseAudio/PipeWire).

Graphics drivers come from the runtime's GL extension. Controllers and other devices are available
(`--device=all`).

## Other differences

- **`.flatpak` releases** need the PC's own Flatpak, which the sandbox can't reach unless the player
  allows it. Players get a message with the command to allow it. Prefer an AppImage or a tarball.
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
