# Building RecompLauncher

The launcher is a [Tauri](https://tauri.app) app: a Rust core (`src-tauri/`) and a window made of
plain HTML, CSS and JavaScript (`ui/`).

## What you need

You need [Rust](https://rustup.rs) 1.85 or newer, Node.js 20 or newer and
[CMake](https://cmake.org). CMake builds SDL 3, which reads controllers; it's compiled into the
launcher, so there is no SDL library to ship or install. On Windows you also need Rust's default
MSVC toolchain, with the Visual Studio C++ build tools it asks for. The Build Tools come with a
CMake of their own if you tick "C++ CMake tools for Windows" in the installer. On Linux you also
need the WebKitGTK libraries (`libwebkit2gtk-4.1-dev`, see the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)) and `libudev-dev`.

On Windows, a checkout in a deep folder can break SDL's build with MSBuild's "could not create
the new file tracking log file" (its paths go past 260 characters). Build with the Ninja
generator instead (`CMAKE_GENERATOR=Ninja`, with `ninja` on your PATH; Visual Studio's CMake
folder has one), or clone to a shorter path.

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

### macOS (initial Apple Silicon support)

Install Xcode Command Line Tools, Rust, CMake (for example `brew install cmake`) and Node.js 20
or newer, then run:

```sh
npm ci
npm test -- --locked
npm run build:macos -- -- --locked
```

This builds `aarch64-apple-darwin`. The application and disk image are in
`src-tauri/target/aarch64-apple-darwin/release/bundle/macos/` and `bundle/dmg/`.
The platform configuration also makes `npm run build` produce an app and disk image on a Mac.
The deployment target is macOS 11; this is a build target, not a claim that every older macOS
release has been tested. Intel launcher builds are not part of this initial support.

The **PR checks** workflow tests and builds on an Apple Silicon `macos-15` runner.
Its `installers-macos-15` artifact contains a ZIP, a DMG and SHA-256 checksums, retained for
14 days. These are test builds: the workflow does not publish releases, and does not use an
Apple Developer ID certificate or notarization. The app is ad-hoc signed. ZIP packaging uses
`ditto` to preserve the application's links and permissions.

For game installation, Mac releases may be a ZIP, a DMG, a RAR or a 7z containing an `.app` or
a native executable. A DMG is mounted read-only, its apps and folders copied out, and unmounted
again; the shortcut to /Applications is skipped. A ZIP is preferred when a release has both.
A download holding one ZIP and no app or program beside it (a readme or a ROM may be) is
treated as a packaging wrapper: the ZIP is unpacked in place, with a bounded nesting depth.
Archives inside the application are left alone. ARM64 and universal game executables are
supported on Apple Silicon; Intel-only games and Windows/Linux games are not translated.
The launcher's own DMG is a separate, supported distribution format.

Choose Program accepts a Mac `.app`; the launcher reads `CFBundleExecutable` from its
`Contents/Info.plist`. A game update replaces the complete app bundle rather than merging its
contents. Saves/settings beside the bundle and in the game's normal Application Support folder
are retained. Changes made inside the application bundle are replaced with the developer's
release; games should keep player data outside their signed bundle.

Launcher updates on Mac download a ZIP or DMG, verify its SHA-256, app identity, version,
architecture and code signature, then replace the complete installed `.app` and reopen it.
Downloads and extraction stay in the app's own data folder. The final verified bundle is copied
beside the installed app so the swap is on the same filesystem. After the Mac event loop closes,
the OS shell runs commands embedded in the launcher and waits for the old process to exit before
replacing or relaunching the app. No helper script or downloaded executable runs from a writable
temporary location. The old bundle is restored if replacement or launch fails, and staging is
cleaned up; a backup is retained only if restoring it also fails.
Run the app from a writable location (such as `~/Applications`), rather than its mounted DMG.
A Windows/Linux-only version feed must not offer an update on Mac. To advertise a Mac release,
the site can add a `macos` entry with
`arch: "arm64"` (or `"universal"`), `url`, `sha256`, and optional `version`, `notes` and `min`.
No Mac release is advertised until that entry exists. The site's release feed and signing
credentials are managed separately from this repository.

In-game screenshot capture, visual effects and platform-specific overlays are not implemented
for Mac in this release. Controller and game compatibility depend on the individual recomp;
a successful launcher build does not establish compatibility for every catalog entry.

Zelda 64: Recompiled v1.2.2 has a known Mac shutdown crash: closing the game can show
"Zelda64Recompiled quit unexpectedly" after gameplay has stopped. This also happens when
the game is launched directly; see [Zelda issue #583](https://github.com/Zelda64Recomp/Zelda64Recomp/issues/583).
Its graphics library has a [merged Metal ownership fix](https://github.com/renderbag/plume/pull/97),
but the published v1.2.2 game predates that fix. Installing or reinstalling the same game release
does not fix it. The launcher preserves the developer's game bundle and does not patch its code.

A build you make yourself won't be byte-for-byte the same file as a release (the compiler stamps
times and paths into it), so its checksum won't match. The code it's built from is this code.

### Testing against a copy of the site

Set `RL_API` to point the launcher somewhere other than recomplauncher.com, for example
`RL_API=http://127.0.0.1:8120 npm run dev`. It reads `/pc-catalog.json` and
`/api/gh.php?repo=owner/name` (the same JSON as GitHub's releases API).

## Pull request builds

The `PR checks` GitHub Actions workflow runs the Rust and UI tests and builds installers on
Windows, Linux and macOS for every pull request, pushes to `main`, and manual runs. Download
the `installers-windows-latest`, `installers-ubuntu-24.04` or `installers-macos-15` artifact from
a successful workflow run for the NSIS `.exe`, Linux `.deb` and `.AppImage`, or Mac app ZIP
and `.dmg` packages. The Mac job builds natively on Apple Silicon, using the default
`src-tauri/target/release/bundle/` output path before staging its artifacts. Artifacts
are retained for 14 days; these builds do not publish a release. Windows CI packages
use Tauri's standard installer screens; the optional branding step above is not run.

A repository administrator must require `Build and test (windows-latest)`,
`Build and test (ubuntu-24.04)` and `Build and test (macos-15)` in the target branch's ruleset
to block merging when any platform check fails. The workflow alone does not enforce merge restrictions.

## How it's put together

- `src-tauri/src/assets.rs` picks which release file is the build for this PC.
- `src-tauri/src/catalog.rs` reads the catalog and releases, with copies on disk.
- `src-tauri/src/install.rs` downloads, checks, unpacks, and finds the program to start.
- `src-tauri/src/library.rs` keeps what's installed and the settings (`library.json`).
- `src-tauri/src/manage.rs` moves games, backs up saves and checks the PC.
- `src-tauri/src/roms.rs` finds the player's own games in their folders.
- `src-tauri/src/shortcuts.rs` makes desktop shortcuts and Steam entries.
- `src-tauri/src/pads.rs` reads controllers through SDL 3 on every system, even while a game is in
  front. A `gamecontrollerdb.txt` in the launcher's data folder (`com.recomplauncher.app` in the
  system's app data folder, or `data` beside the program in portable mode) adds SDL mappings for
  controllers SDL doesn't know.
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
