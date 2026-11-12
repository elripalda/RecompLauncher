<p align="center"><img src="docs/images/banner.jpg" alt="RecompLauncher: PC recomps, all in one place." width="100%"></p>

<p align="center">
  <a href="https://github.com/elripalda/RecompLauncher/releases/latest"><b>Download the beta</b></a> ·
  <a href="https://recomplauncher.com">recomplauncher.com</a> ·
  <a href="https://discord.gg/FUzqGM2cvf">Discord</a>
</p>

<p align="center">
  <a href="https://github.com/elripalda/RecompLauncher/stargazers"><img src="https://img.shields.io/github/stars/elripalda/RecompLauncher?style=flat&color=34f2e8&label=Stars" alt="Stars"></a>
  <a href="https://github.com/elripalda/RecompLauncher/releases"><img src="https://img.shields.io/github/downloads/elripalda/RecompLauncher/total?style=flat&color=34f2e8&label=Downloads" alt="Downloads"></a>
  <a href="https://github.com/elripalda/RecompLauncher/releases"><img src="https://img.shields.io/github/v/release/elripalda/RecompLauncher?include_prereleases&style=flat&color=34f2e8&label=Latest" alt="Latest version"></a>
</p>

---

RecompLauncher finds, installs, updates and plays PC recomps: native PC versions of console
games, made by their developers from the original game. You bring your own copy of the game and
the launcher sets it up.

## What's in it

- **Discover:** every recomp on recomplauncher.com, with screenshots, what it needs and how it
  runs for other players.
- **Coming Soon:** the recomps on the way, with Notify Me for an email when each comes out, and
  Request a Recomp to ask for a project on GitHub or GitLab.
- **Install and update in one click,** straight from the developer's own GitHub release. Every
  download is checked against GitHub's checksum.
- **Easy Setup:** point it at your disc image or game folder and it does the rest, for the recomps
  that support it.
- **Versions:** games with more than one recomp show once, and you can switch between versions or
  keep a few side by side.
- **Saves:** backups before every update, and cloud saves with a free account.
- **Big Picture mode:** a full-screen view for a controller and a TV.
- **Looks:** CRT and other screen effects, per game.
- **Player reports:** see how a recomp runs for people before you install it, and add your own.
- **Verified developers:** follow the people making the recomps you like and hear when they put
  out something new.

## A look around

<p align="center"><img src="docs/images/home.jpg" alt="Home" width="100%"></p>

<table>
  <tr>
    <td width="50%"><img src="docs/images/discover.jpg" alt="Discover"><p align="center"><b>Discover</b></p></td>
    <td width="50%"><img src="docs/images/game.jpg" alt="A game's page"><p align="center"><b>A game's page</b></p></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/bigpicture.jpg" alt="Big Picture mode"><p align="center"><b>Big Picture mode</b></p></td>
    <td width="50%"><img src="docs/images/saves.jpg" alt="Saves"><p align="center"><b>Saves</b></p></td>
  </tr>
</table>

## Get it

Windows 10 or 11, 64-bit. Initial macOS support targets Apple Silicon: see the
[Mac build and test-artifact instructions](docs/BUILDING.md#macos-initial-apple-silicon-support).
Mac test builds support native Mac games packaged as ZIP, DMG, RAR or 7z and are not Apple-notarized releases.

Download the setup file from [Releases](https://github.com/elripalda/RecompLauncher/releases/latest)
and run it. While the beta is new, Windows may say "Windows protected your PC". Click
**More info**, then **Run anyway**. Each release lists its file's SHA-256 so you can check you
have the real one.

In **Settings > General > When you close the window**, **Run in background** controls whether
closing the window keeps RecompLauncher in the system tray. Turn it off to exit the launcher
and its background work when you close the window. **Quit RecompLauncher** always exits.
The setting is also available in Big Picture mode. It is off by default for new installations;
existing saved choices are kept. Games already running keep playing.

## No game files

RecompLauncher has no game files and never downloads any. Every recomp comes from its developer's
GitHub, and you set it up with your own copy of the game.

## Found a bug? Have an idea?

The quickest way is **Settings > Beta** in the launcher. It goes straight to the developer and can
include your PC's details. You can also post in the [Discord](https://discord.gg/FUzqGM2cvf) or
[open an issue](https://github.com/elripalda/RecompLauncher/issues/new/choose) here.

If a game crashes, send the log from the crash screen. Every one helps.

## Making a recomp?

Add a `recomplauncher.json` to your release and players get one-click setup, your settings inside
the launcher, and Big Picture mode support. The guide is in [docs/DEVELOPERS.md](docs/DEVELOPERS.md),
with examples in [docs/examples](docs/examples).

## Source code

The full source of the launcher is here: the Rust core in `src-tauri/` and the window in `ui/`. You
can read exactly what it does, or build it yourself with the steps in
[docs/BUILDING.md](docs/BUILDING.md). It's licensed under the GPL v3 ([LICENSE](LICENSE)).

## Stars over time

<a href="https://star-history.com/#elripalda/RecompLauncher&Date"><img src="https://api.star-history.com/svg?repos=elripalda/RecompLauncher&type=Date&theme=dark" alt="RecompLauncher's stars over time" width="100%"></a>

## About

Made by [Ripalda](https://github.com/elripalda). If you'd like to support it: [Ko-fi](https://ko-fi.com/recomp).

RecompLauncher is free to download and use. It isn't affiliated with or endorsed by any game's
makers or platform holders.
