# Desktop engine integration

The test build can install an engine-backed game through the existing Easy Install and
named Easy Setup screens. It downloads the pinned components, reads the selected files,
prepares a standalone native app, validates it, and records its engine pin. Play starts that
recorded app without invoking preparation. Engine details are optional in the existing
version/update views. No engine wizard or manual engine selection is required.

Maintainers updating **the engine or its game profiles** must follow
[Updating engines and game profiles](UPDATING-ENGINES.md). That guide covers checkpoint changes,
catalog regeneration, dependency verification, per-game pinning and update validation.

## Try it

Build the isolated test app from the working branch:

```sh
npm ci
npm run build -- --debug --bundles app --config src-tauri/tauri.switch-test.conf.json -- --locked
```

Open `src-tauri/target/debug/bundle/macos/RecompLauncher Switch Test.app`. Its separate application
identifier keeps the test library/settings under `~/Library/Application Support/com.recomplauncher.switchtest`.
It does not migrate the regular launcher's library. Actual game saves use nxnative's normal
per-title save folder, which may already be shared with other nxnative frontends.

1. Find a game in Discover. Engine entries include their upstream profile version and notes.
2. Choose Easy Install and use the existing setup screen to select the base game file and its
   matching update, if separate. Import `prod.keys` there when the selected Switch 1 format
   requires it; the launcher reuses its private copy for later games.
3. Start setup. Downloading components, extracting, preparing shaders, compiling, and validating
   appear as progress. Cancellation stops the owned process group. Retry keeps matching work.
4. Press Play after preparation finishes.

Engine setup shows nxnative's measured current-step progress, work counts, stage number, elapsed
stage time and estimated remaining time for that step in both desktop and Big Picture. Unknown
work shows an indeterminate bar. A new step resets its measurements; estimated counts are marked
with `~`. The display does not treat one step's ETA as the whole installation's remaining time,
and final validation/registration must still complete before the game is ready.

Apple silicon, Xcode with a macOS SDK, and the engine's supported Metal compiler are prerequisites.
The app checks Xcode/Metal before setup and the engine checks its exact compiler lock. No global
Homebrew, Python, pip or .NET setup is required. Components are installed privately on demand.

This checkpoint accepts Switch 1 NSP/XCI/NSZ/XCZ plus an optional separate update, and NUP files
or extracted folders for the profiles that support them. The folder input means an extracted
copy, not a directory containing encrypted packages. Extracted updates belong inside that folder;
combining an extracted folder with a separate encrypted update is rejected. Separate DLC selection and Switch 2
key management are not offered. Unsupported/mismatched revisions fail without registering an app.

## Package and catalog provenance

The Mac adapter consumes the source archive directly from
[seedds/nxnative](https://github.com/seedds/nxnative/tree/9ccbd6b1a152bc8449b092ab60999be13c2971aa).
`src-tauri/src/engines/adapters/nxnative/nxnative-components.json` records its exact commit, archive SHA256, and
every independently obtained build dependency: standalone Python from Astral, exact wheels from
PyPI, .NET SDK from Microsoft, and native tools from Homebrew's public bottle registry. The
launcher has no dependency on another frontend's application or tool distribution.

Downloads are checksum-verified before extraction or execution. Python and native tools are
assembled into private versioned storage, native library references are relocated, and wheels
are installed offline from verified files. No global Homebrew or Python environment is used.
The component manifest and bootstrap script together identify the private toolchain; the final
game pin also records the Apple tool versions. Cached packages retain their licenses. Each
preparation uses a separate engine source copy because nxnative writes generated tools there.

`catalog-nxnative.json` snapshots game profiles and notes from that same source commit.
The parser imports title, console, preferred package/version and main build ID. Profiles without
those identities are omitted. Listings are experimental: an upstream profile is evidence of an
integration route, not a blanket guarantee of gameplay. `artwork-nxnative.json` maps exact title
IDs to Nintendo images using Nintendo and public title metadata, independently of build pins.
Artwork uses the existing media cache; refreshing it does not rebuild games.

Adapters are discovered automatically from their folders and filtered by their declared host
targets. They declare supported game systems independently of host platforms. See the
[contributor contract](ADDING-ENGINES.md); a new engine does not need a name branch in shared
installation code. nxnative supports Mac; ScarletNX supplies an experimental Windows/Linux route.

## Persistence and recovery

- Pending installations store their game profile and package checkpoint in `engine_build` and
  use existing named-input selection. Explicitly retrying an unfinished setup may adopt the
  current nxnative checkpoint; prepared games and completed results waiting for registration
  keep their original pins. They cannot fall through to an ordinary release installer
  or launch. Prepared installations retain their frozen native profile and save paths.
- `.engine-work/<package-and-input-fingerprint>/job.json` records each attempt. The builder's
  matching extraction/shader work is retained after interruption; retry needs a user action.
  A Python parent watcher stops the build group if the launcher dies.
- The output is staged separately. Title/version, main NSO build, native executable, bundle links
  and signature are checked before activation. A completed-result record allows library
  registration to recover after interruption before or after the final file move without repeating
  compilation. Cancellation and installation ownership are checked again at registration. Move
  and uninstall cannot overlap preparation, and registration retains current player preferences.
  A completed result waiting for registration must finish Setup before Move becomes available;
  registration reuses the completed app without compiling it again. Registry downloads have
  cancellable waits and inactivity deadlines, including token responses and body transfers.
- Catalog refreshes, ordinary updates, automatic-update settings and Play cannot reprepare an
  existing build. This checkpoint exposes no Apply Fix or automatic migration endpoint.
- Launch uses the pinned executable directly. Save backup/restore uses the frozen profile or an
  explicit player choice; caches and source/key files are not guessed as saves.

These are internal library/catalog contracts, not new developer-facing `recomplauncher.json` fields.

## Validation and remaining scope

A local acceptance test prepared Hollow Knight 1.5.12620 from NSP base/update files, produced a
Mac app, verified its signature and exact launch target, and registered its pinned record in an
isolated test folder. A second run recovered the same completed app without rebuilding it.
A separate 35-second startup smoke test ran the prepared host with isolated temporary saves,
rendered more than 300 frames with audio callbacks, and exited cleanly when stopped. This is
startup evidence, not a gameplay or performance certification; interactive testing remains
with the player. No proprietary game files or keys are included in the repository.

```sh
node --test tests/*.test.cjs
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

`real_nsp_preparation` is an opt-in acceptance test. It consumes a local JSON fixture named by
`RL_ENGINE_FIXTURE` with `root`, `title_id`, `base`, optional `update`, optional `keys`, and optional
`archive` fields. Every write stays under `root`; it does not use the live launcher library.

Remaining proposal work: Windows/Linux gameplay validation and durable package hosting, dynamic authenticated package/catalog updates,
additional engine alternatives, explicit relevant-fix jobs with save backup and rollback,
component-specific reuse, prepared-installation import, separate DLC inputs, and broader gameplay
validation. These are not represented as shipped features in the test build.


## Windows and Linux testing

The ScarletNX adapter targets Windows x86-64 and Linux x86-64 with Vulkan 1.3. It uses verified
packages from [the pinned nightly](https://github.com/scarletnx/nightly/releases/tag/nightly-2026-11-13-2d5ec80),
corresponding to [source commit 2d5ec80](https://github.com/scarletnx/ScarletNX/tree/2d5ec80b53b5da4dde80ae2821e5edbe7804fd17).
It does not use nxnative or NX2Apple on those targets. Other candidates and their limitations are
recorded in [Desktop engine candidates](DESKTOP-ENGINE-CANDIDATES.md).

Build from the PR branch on the target computer, with the project's normal Rust/Tauri build
prerequisites installed. Use the test configuration to keep the launcher's test library separate:

```sh
npm ci
# Windows (PowerShell or a developer terminal):
npm run build -- --debug --bundles nsis --config src-tauri/tauri.switch-test.conf.json -- --locked
# Linux:
npm run build -- --debug --bundles appimage --config src-tauri/tauri.switch-test.conf.json -- --locked
```

The installers are under `src-tauri/target/debug/bundle/nsis/` or `appimage/`. The test configuration
uses `com.recomplauncher.switchtest`; it does not migrate the normal launcher's library.

Use the same Discover → Easy Install → select files → Setup → Play flow. Windows/Linux Switch 1
listings are explicitly **unverified**, using title/artwork metadata rather than a claim that
ScarletNX runs every listed title. Switch 2 is not offered by this adapter. Accepted inputs are
NSP/XCI, decrypted DNSP/DXCI, or an extracted folder containing `exefs/main.npdm`, `exefs/main`,
`romfs` and `control.nacp`. Select a separate update only with a packaged base; extracted folders
must already include it. NSP/XCI inputs explicitly require the existing shared `prod.keys` input.
NUP/NSZ/XCZ and separate DLC are not supported by this adapter.

Setup downloads the pinned engine, extracts and identifies the selected game, compiles it and
validates the output before making Play available. ScarletNX currently reports activity/stages
without a measured overall percentage/ETA. Linux keeps the complete AppImage and uses its
extraction mode, avoiding a FUSE requirement. Windows retains the engine's DLLs and licenses.
Play uses the game's own runtime with `--game`, never the preparation driver. Each game has a
portable `.scarletnx` root for its runtime, game data, users, settings and saves; backups include
only the saved profile's player-data paths. Engine/source/key files and caches are excluded.

No Windows/Linux game device was available to the contributor. Community testing is needed for
fresh installation, real game preparation, Play, controllers, save/reload, moved installations,
and Stop/retry on each platform. Report OS, GPU/driver, game/version, pinned engine version,
reproduction steps and relevant preparation-log errors; do not upload games or key files.
Keep the PR draft until this testing and durable package availability are resolved. A passing
native CLI smoke test establishes package startup, not game compatibility.

For a native package check without game files:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked --test scarlet_engine pinned_desktop_package_starts_its_cli -- --ignored
```

The Mac Hollow Knight build was also tested interactively by the contributor and reported to
build and run perfectly. Controller integration and saves still need explicit testing.
