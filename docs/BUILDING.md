# Building RecompLauncher

The launcher is a [Tauri](https://tauri.app) app: a Rust core (`src-tauri/`) and a window made of
plain HTML, CSS and JavaScript (`ui/`).

## What you need

You need [Rust](https://rustup.rs) and Node.js 20 or newer. On Windows that's all (Rust's default
MSVC toolchain, with the Visual Studio C++ build tools it asks for). On Linux you also need the
WebKitGTK libraries (`libwebkit2gtk-4.1-dev`, see the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)).

```
npm ci
npm run dev        # run it
npm test           # the core's tests
npm run build      # installer for this system
```

On Windows, `npm run build` makes the installer in `src-tauri/target/release/bundle/nsis/`. The
released installer also has RecompLauncher's own install screens; to add them, install
[NSIS](https://nsis.sourceforge.io) (with its folder on your PATH) and run `python tools/brand-installer.py src-tauri/target/release`
after the build.

A build you make yourself won't be byte-for-byte the same file as a release (the compiler stamps
times and paths into it), so its checksum won't match. The code it's built from is this code.

### Testing against a copy of the site

Set `RL_API` to point the launcher somewhere other than recomplauncher.com, for example
`RL_API=http://127.0.0.1:8120 npm run dev`. It reads `/pc-catalog.json` and
`/api/gh.php?repo=owner/name` (the same JSON as GitHub's releases API).

## How it's put together

- `src-tauri/src/assets.rs` picks which release file is the build for this PC.
- `src-tauri/src/catalog.rs` reads the catalog and releases, with copies on disk.
- `src-tauri/src/install.rs` downloads, checks, unpacks, and finds the program to start.
- `src-tauri/src/library.rs` keeps what's installed and the settings (`library.json`).
- `src-tauri/src/manage.rs` moves games, backs up saves and checks the PC.
- `src-tauri/src/roms.rs` finds the player's own games in their folders.
- `src-tauri/src/shortcuts.rs` makes desktop shortcuts and Steam entries.
- `src-tauri/src/pads.rs` reads controllers (XInput, and PlayStation pads directly).
- `src-tauri/src/media.rs` keeps cover art on disk and copies screenshots to the clipboard.
- `src-tauri/src/ingame.rs` handles the quick menu, hotkeys and taking screenshots.
- `src-tauri/src/mods.rs` installs mods from Thunderstore and `gamebanana.rs` from GameBanana;
  `info.rs` reads Wikipedia; `discord.rs` sets the Discord status.
- `src-tauri/src/files.rs` lists drives and folders for the launcher's file browser.
- `src-tauri/src/saves.rs` finds where each game saves and keeps copies; `cloud.rs` signs the PC in to a
  RecompStore account and sends copies to it ([LAUNCHER-API.md](LAUNCHER-API.md) has the website's side).
- `src-tauri/src/tray.rs` is the icon by the clock.
- `src-tauri/src/profile.rs` reads a recomp's `recomplauncher.json` (examples are in
  [`docs/examples/`](examples)): how to start it with the player's copy and which settings to show.
- `src-tauri/src/looks.rs` gets ReShade, writes a game's look, and puts ReShade where the game loads it
  for one start. The effects are in `src-tauri/looks/`; `ui/looks.js` draws the same ones for the previews.
- `src-tauri/src/lib.rs` has the commands the window calls, and starts games.
- `ui/` is the window (app.js), Big Picture mode (tv.js), first-time setup (firstrun.js), accounts and the Saves tab (account.js) and the quick menu
  (overlay.html): plain HTML, CSS and JavaScript, no build step.
- `ui/i18n.js` puts the window in the player's language, from `ui/i18n/<code>.json`.
  `python3 tools/launcher-i18n.py` lists every text and checks the translations.

## License

GPL-3.0-or-later. See [LICENSE](../LICENSE). Each recomp has its own license, set by its developer.
