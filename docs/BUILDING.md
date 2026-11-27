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

### Arch Linux (native x86_64 package)

Use `bash tools/build-arch-package.sh --noconfirm` to build a native pacman package
from this checkout. The result is in `dist/arch/`; install it with
`sudo pacman -U dist/arch/recomplauncher-0.90.0beta.42-1-x86_64.pkg.tar.zst`.
See [the Arch packaging guide](../packaging/arch/README.md) for dependencies,
version spelling, rebuilding and updates. The Arch CI job also adds this format
to release drafts. Flatpak has its own [build guide](../packaging/flatpak/README.md).

### Nix and NixOS

The repository is a flake: `nix build` builds the launcher, and `nix develop` opens a shell with
everything `npm run dev` needs. See [the Nix packaging guide](../packaging/nix/README.md). The
`Nix` workflow builds it on pull requests that touch it; like the Flatpak build, it isn't a
required check.

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

The launcher updates itself on Mac from the release's DMG: Update Now downloads it, checks its
`sha256`, mounts it read-only, checks that the app inside is the version on offer and that its
ad hoc signature verifies (an app that fails either check is refused), swaps it in for the running
RecompLauncher.app and opens it again. The previous app is kept aside until the new one has run
for 20 seconds; if the new one crashes or macOS stops it before then, the previous one is put
back and opens, and it tells the player the update didn't start. Twice from the same file and
that update isn't offered again. When the running app's folder can't be written (it runs
from the DMG itself, from a folder macOS moved it to, or from one only an administrator can
change), or the feed's file isn't a `.dmg`, the DMG link opens instead for a manual install.
A Windows/Linux-only version feed must not offer an update on Mac. To advertise a Mac release,
the site can add a `macos` entry with `arch: "arm64"` (or `"universal"`), `url`, `sha256`, and
optional `version`, `notes` and `min`. No Mac release is advertised until that entry exists.
The site's release feed and signing credentials are managed separately from this repository.

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

### Building and running with a boundary around it

The Flatpak build already runs inside a sandbox with a fixed list of what it may reach. `sandbox/`
is the same idea for the other builds — a `.deb`, an AppImage, or this checkout — with the list in
a JSON file you can read:

- `sandbox/build-in-container.sh` builds the Linux bundles in a container that sees this checkout
  read-only, with the pass that produces the artifacts running with the network off;
- `sandbox/rl-sandbox` starts the launcher under bubblewrap with only the folders named in a small
  JSON config — the game library and disc images you list, and nothing else from your home.

It is optional and Linux-only, and the commands above are still the normal way to build. See
[sandbox/README.md](../sandbox/README.md), which includes scripts that check the boundary instead of
assuming it.

### Testing against a copy of the site

Set `RL_API` to point the launcher somewhere other than recomplauncher.com, for example
`RL_API=http://127.0.0.1:8120 npm run dev`. It reads `/pc-catalog.json` and
`/api/gh.php?repo=owner/name` (the same JSON as GitHub's releases API).

### Testing a build with an agent

Start the launcher with `RECOMPLAUNCHER_TEST_CONTROL` set to a token of your own, and an MCP client
(an AI agent, or a script) can drive it: see [TEST-CONTROL.md](TEST-CONTROL.md). It's off otherwise.

## Android (test builds)

The Android app is in early testing and isn't released. It needs Android 8.0 or newer (the RAR unpacker and installing other apps both need it). It's the same Rust core and window as
the PC launcher, built with Tauri's Android support; the Android Studio project is
`src-tauri/gen/android`.

The Android build leaves out what only a PC has: the tray icon, global shortcuts, a second copy
handing over to the first, Discord status and the picture clipboard. Controllers are read by the
window (`navigator.getGamepads`) instead of SDL. Window calls a phone doesn't have (minimise,
full screen, frameless windows) do nothing there; `src-tauri/src/mobile.rs` keeps the shared code
the same for both. `pads_mobile.rs` and `tray_mobile.rs` stand in for `pads.rs` and `tray.rs`.

To build it yourself you need Android Studio (its SDK and NDK), Java 17 and the Rust target:

```
rustup target add aarch64-linux-android
export ANDROID_HOME=<your Android SDK>   NDK_HOME=$ANDROID_HOME/ndk/<version>
npx tauri android build --debug --apk --target aarch64
```

On Android there's no desktop view. There are two layouts instead:

- **The phone layout** (`ui/phone.js`, `ui/phone.css`) when no controller is in use: an app made for
  fingers, upright or sideways. Home, Catalog, Library and Settings are tabs on a floating menu (at
  the bottom upright, down the side away from the camera sideways); the player picks 3 to 5 of them
  in Settings, and can add Downloads or Coming Soon. A swipe moves between tabs, a cover grows into
  its game page, and pages inside pages slide in from the right (a swipe from the left edge goes
  back). Get installs in place, like an app store, and turns into Open. No button hints. Android's
  Back closes the topmost thing (a sheet, search, the game page, a page inside a page), then goes
  to Home, and from Home puts the launcher in the background.
  The design is drawn 433 points wide and scaled to the screen (Settings, Display size: small,
  medium or large), so a tablet or a handheld gets the same layout, bigger. Android tells the page
  how tall the status bar and gesture line are (`android_insets`, and again whenever they change),
  the page draws behind them, and nothing readable sits under the clock or the camera. The bars'
  icons follow the theme (`android_edges`), and taps can buzz (`android_haptic`, Settings, Vibrate
  on taps). The player's choices are kept in settings.ui (`phone_tabs`, `phone_labels`,
  `phone_haptics`, `phone_size`, `phone_libview`, `pad_tv`).
- **Big Picture mode** (`ui/tv.js`) with a controller, held sideways. Any button pressed on a
  controller switches to it, unless Settings, Open with a controller is off (Android tells the
  page about a controller only once a button is pressed), and when the last controller is unplugged it goes back to the phone layout. It's also
  in the phone layout's Settings. Settings leaves out what only a PC has (monitor, tray, start with
  your PC, Discord, controller as a mouse), and Leave Big Picture mode goes to the phone layout. A
  finger drags a page or a row of games, a tap opens, and Android's Back is B. On a phone, Auto size
  draws the stage a little bigger (1600×900) so words stay readable.

A device without a touch screen (an Android TV) always uses Big Picture mode. Which way the window
turns follows the layout: Big Picture mode only sideways, the phone layout as the phone is held and
as its rotation lock says (`ScreenPlugin.kt`). Nothing is drawn under the camera hole
(`MainActivity.kt`). Cloud saves aren't offered: a phone's games are Android apps, and their saves
are out of the launcher's reach.

The desktop view isn't drawn at all on Android: `ui/droid.js` marks the page from `<head>` so it
never shows at start-up, and `render()` builds nothing for it. Android also draws lighter by default
(no moving backdrop, no blurs), as a handheld PC does. The phone layout patches the page that's
showing instead of rebuilding it, and each page keeps its place when you come back to it.

Android's web engine (Android System WebView) can be years old on a TV box, an emulator or a phone
that isn't updated. Styles that an older engine doesn't know need a plain version beside them:
`tv.css` ends with one for every colour-mixing rule (a test checks none is missing), and
`phone.css` has one for shapes. Settings → About in the phone layout shows the engine's version.

The launcher sets nothing up for an Android game: no Easy Setup, no Game Settings, no checks of
the PC's parts. A game there is an app of its own, and Android keeps its files out of every other
app's reach, so the launcher can't put the player's copy where the game reads it. Play opens the
game, and the game asks for what it needs itself (`profile::find` gives no profile on Android).

Its game list is the website's Android list (`/api/pc.php?a=android`: the same shape as
`pc-catalog.json`, with each game's Android download), not `pc-catalog.json`. While Android is
hidden on the website only testers get it, so the request carries the player's sign-in; signed
out, or with an account that isn't a tester, Home says so and offers Sign In. Each game's release
and version are its Android app's (`android_tag`, `android_version`), so updates work as on PC.

Install downloads the game's APK from that release (the file `assets.android` names), checks it is
the app the store checked (`android_package`, and the SHA-256 of its signing certificate against
`android_sig`), and hands it to Android's own installer, which asks the player to confirm
(`src-tauri/src/android.rs`, `ApkPlugin.kt`). Play opens the app, and Uninstall asks Android to
remove it. The Library follows what's installed on the device: it's checked at start and whenever
the launcher comes back to the front, so a game installed or removed outside the launcher shows
right. The first install asks the player to let RecompLauncher install apps.

The Android workflow (`.github/workflows/android.yml`) builds the same APK on GitHub when the
Android parts change, or when run by hand, and keeps it for a week (time to put it on a phone). It also opens the APK on an
Android emulator and prints a few small screenshots (base64 JPEG between `=====SHOT` lines) and
the app's log into the job's log (`.github/scripts/android-smoke.sh`).

## Pull request builds

The `PR checks` GitHub Actions workflow runs the Rust and UI tests and builds installers on
Windows, Linux and macOS for every pull request, pushes to `main`, and manual runs. Download
the `installers-windows-latest`, `installers-ubuntu-24.04` or `installers-macos-15` artifact from
a successful workflow run for the NSIS `.exe`, Linux `.deb` and `.AppImage`, or Mac app ZIP
and `.dmg` packages. The Mac job builds natively on Apple Silicon, using the default
`src-tauri/target/release/bundle/` output path before staging its artifacts. Artifacts
are retained for 14 days; these builds do not publish a release. Windows CI packages
use Tauri's standard installer screens; the optional branding step above is not run.

A separate `Arch Linux package` reusable workflow builds a native x86_64 `.pkg.tar.zst`
on Arch Linux for these events. Manual runs retain `installers-arch-x86_64` for three
days. Release builds include the package in the draft; updates use `pacman -U`.

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
- `src-tauri/src/test_control/` is the MCP server for testing a build with an agent, off unless
  it's asked for ([TEST-CONTROL.md](TEST-CONTROL.md)).
- `ui/` is the window (app.js), Big Picture mode (tv.js), first-time setup (firstrun.js), accounts and the Saves tab (account.js) and the quick menu
  (overlay.html): plain HTML, CSS and JavaScript, no build step.
- `ui/i18n.js` puts the window in the player's language, from `ui/i18n/<code>.json`.
  `python3 tools/launcher-i18n.py` lists every text and checks the translations.

## License

GPL-3.0-or-later. See [LICENSE](../LICENSE). Each recomp has its own license, set by its developer.
