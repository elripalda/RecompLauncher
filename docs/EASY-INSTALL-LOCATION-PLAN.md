# Easy Install: choose the game's installation folder

Status: draft implementation with both rounds of review findings addressed below. Existing manifest
fields and `{install}` support the selected folder; no manifest extension was added. External builders
retain their declared output paths until their integrations expose a supported destination option.

The implementation includes a desktop/controller picker, exact-target validation, persisted pending
requests, owned work directories, retry/cancel recovery and dedicated-folder ownership for uninstall.
Setup shows the installation and any separately declared build location. Recognized external
installers require acceptance when their actual destination differs.

Validation covers destination and recovery tests, a real archive → preparation → launch → move flow
on Linux, frontend request/controller tests, the full Rust and JavaScript suites, and a desktop picker
render check. Native Windows/macOS installers and Flatpak portal permissions still need platform
smoke testing. The repository-wide locale checker has the same 105 pre-existing errors as `HEAD`;
the new strings add no errors.

The bundled boot profiles declare no `{home}/Games` or destination-option strings to migrate. Profiles
supplied by a catalog or an upstream release are external rollout dependencies. They are not rewritten
automatically, and existing external builds remain in place. The sections below preserve the design
and acceptance criteria for future integration updates.

## P2 review fixes

The follow-up fixes all four findings and adds regression coverage for the reported scenarios.

- [x] **Keep work directories within the granted filesystem scope.** Downloads and extraction use
  owned directories under the launcher's private `cache/install-work/`, without creating anything
  beside the selected target. Write checks on existing targets do not require access to the parent.
  Tests cover a folder-only grant model (a parent without read/list/write permission) and a writable
  game folder beneath a read-only parent. Cache-drive space is checked separately. Mac bundles are
  staged on the target volume before swapping, so a cache on another volume preserves app updates.
- [x] **Preserve backup provenance when resuming an automatic release.** Pending requests retain
  the checked backup release, including asset URLs and SHA-256 digests, before build selection or
  downloading. Pause/restart, failure/retry and build selection reuse that source without querying
  a removed upstream project. Selecting a different explicit release clears the stored backup.
- [x] **Clear retained arguments when cancelling a paused install.** Desktop and Big Picture use
  one cancellation helper. After backend cancellation succeeds, it clears the paused state, saved
  frontend request, pending request and preselected source, without waiting for a `cancelled` event.
  Tests cancel a paused older release and then start a fresh install; failed cancellation keeps the
  resumable state.
- [x] **Use the parent recomp ID for the desktop setup folder button.** The button passes
  `parentId(id)`, matching Big Picture. Tests exercise folder opening from both legacy and named-input
  child-game setup views.

The earlier macOS lifecycle-test failure was a fixture using an unnormalized `/var` temporary path;
that fixture now uses the same normalization as production destinations. Native platform smoke tests
remain separate from these automated checks.

## Ownership and recovery follow-up

The second review identified one P1 and three P2 issues, addressed with additional regression tests:

- [x] **Relinking does not transfer deletion permission.** A replacement location is checked for
  overlaps with installed games after normalizing filesystem aliases. The relinked record clears
  managed ownership and explicitly preserves its folder, even when legacy library/name heuristics
  would otherwise allow deletion. The protection is saved and carried through updates; desktop
  and Big Picture do not offer to delete the folder. Relinking also clears work for the old path.
- [x] **Uninstall and forget clear recovery state.** All removal paths discard the pending request
  and only its owned work. Library removal also clears reservations when no installed record exists.
  Frontend refresh drops cached update arguments when the installed record is removed. A failed
  update followed by uninstall/reinstall no longer reuses an anchor pointing at the deleted game.
- [x] **Explicit newest differs from retry.** Choosing a version, including newest, sends an explicit
  release reset. It clears the previous tag, selected asset and backup metadata while retaining the
  folder's ownership token. Location confirmation preserves that intent; subsequent build choices
  and resumes retain the new selection without repeating the reset.
- [x] **Cross-volume updates safely replace read-only files.** Regular files are copied to a private
  temporary location on the destination volume and replaced by rename, rather than copying over
  the existing file. Failed copies leave the previous file intact. A real cross-filesystem tar.gz
  test installs and updates a mode-0444 file and checks that saves and file permissions remain.

The player should choose the final game folder before an Easy Install begins. The release,
launcher-managed game content, and supported setup/build output should stay under that folder.
Updates, retries and later setup must use the same location.

## What exists today

- `ui/app.js`: `startInstall` already opens `locationModal` on the first install, when additional
  libraries exist, or when Settings → Ask Where to Install is enabled. This includes Easy Install.
- The picker selects a **library parent**. `do_install` in `src-tauri/src/lib.rs` appends
  `library::folder_name(id)` to it. It does not accept an exact game folder today.
- Several install actions in `ui/tv.js` pass the default library explicitly, which bypasses the
  location prompt. `pickBuild` in `ui/app.js` restarts installation with `dir: null, tag: null`,
  losing both the chosen library and a requested release.
- Installed entries persist the release location as `Installed.path`. Updates already reuse it.
- A profile can build elsewhere using paths such as `{home}/Games/Game`. `build_dir` and
  `Profile::rebase_build` support playing an existing build elsewhere; they deliberately do not
  redirect the prepare step. Reusing that field for an installation destination would be incorrect.
- Windows installer releases suggest `Installed.path` to recognized installers, then discover
  where the game actually went. An installer can still use a different location.

## Proposed player experience

1. Pressing **Easy Install** for a new recomp opens **Where should [game] go?**, on desktop and
   in Big Picture, before downloading or selecting the player's source copy.
2. Preselect the current default library and show the complete resulting game path, for example
   `D:\Recomp Library\example-recomp`. Keep the existing folder-name convention.
3. Offer the existing library choices plus **Choose game folder…**. This chooses the exact game
   folder, for example `E:\Games\My Recomp`; installation must not append another folder name.
   Allow a new folder or an existing empty folder. Do not require adding its parent as a library.
4. Confirm with **Install here**. Cancel leaves the download unstarted. A one-game selection
   does not change the global default or silently register another library.
5. Show the chosen location in subsequent setup and the game's management view, with **Open
   Folder**. Auto-Install on/off changes when setup starts, not where installation goes.
6. Updates and repair/retry actions reuse the existing location. Changing location after install
   uses the existing Move flow. Multiple games carried by one recomp share its install location.

Keep the existing Ask Where to Install preference for ordinary installs. New Easy Installs always
show this confirmation; adjust the preference's help text to make that distinction clear.

“Game files” means the release and content installed/generated for it. Original ROMs, discs,
named source paths, shared key masters and launcher metadata keep their existing storage rules.
Games may still keep saves/settings in their documented system folders. Do not promise that an
arbitrary third-party installer or builder can be forced into a folder it does not support.

## Implementation sequence

### 1. Resolve and validate one final destination in Rust

- Add an optional exact `target` argument to the install command. Preserve `dir` as the existing
  library-parent argument for compatibility; reject requests supplying both. Resolve either form
  once, before the install starts. Keep this logic in a small helper used by preview and execution.
- For installed games, continue using `Installed.path`; reject a conflicting target and direct
  location changes through Move. Keep release/version/asset selection independent of destination.
- Normalize absolute paths and compare actual filesystem locations, including Windows case and
  symlink/junction aliases. Reject overlapping installs, launcher data/work directories, source
  content directories and filesystem roots. For new installs, reject nonempty destinations and
  offer **I Already Have It** for adopting an existing build instead of overwriting it.
- Probe write access and available space on the selected volume. Recheck before writing, including
  after an external drive disappears. Unknown free space must not be reported as sufficient.
- Allocate isolated download/extraction work areas in the launcher's private data folder. This
  keeps work within the launcher's filesystem grant even when the game picker grants only the
  target. Check space on both drives, preserve archive traversal protections and only remove owned
  work paths. Complete Mac app bundles on the target drive before an atomic replacement.
- A failed first attempt needs enough persisted pending-install state to retry into its own partial
  folder without treating it as an unrelated nonempty folder. Record ownership and target before
  writing game content; never recursively delete a selected pre-existing folder during cleanup.

### 2. Carry the choice through every frontend path

- Extend `locationModal` to preview the resolved final directory and browse an exact target.
  Reuse the file-picker abstraction, including Flatpak portal access and controller navigation.
- Use one install-request object containing destination, tag and selected asset. Preserve it through
  the source-copy prompt, `PICK_BUILD`, paused-download resume, errors and retries. Persist the
  pending destination so restarting the launcher cannot redirect an unfinished install.
- Update Big Picture's new-install actions to enter the common selection flow. Give the dialog
  controller focus, scrolling and cancellation behavior consistent with other Big Picture dialogs.
- Audit older-release installs, children of multi-game recomps and Auto-Install off. A selected
  release must survive the location dialog just as the location must survive the build dialog.
- Add translated strings through the existing localization workflow.

### 3. Keep preparation output with the installation

- Audit Easy Install integrations for output paths outside `{install}`. For builders with a
  supported destination option, update their profile to pass an `{install}`-relative output folder
  and use that same path in `start`, `prepare.done`, config, saves and folder declarations.
  Keep release tools and generated content in separate subfolders when the builder needs that.
- Prefer the existing `{install}` placeholder and profile fields. Do not rewrite `{home}` globally,
  guess command-line options, or treat selecting an existing `build_dir` as redirecting a build.
- Verify the builder's real destination option before changing each integration. Profiles supplied
  by the catalog or upstream release need corresponding updates; changing this repository alone
  cannot redirect those builders. Track those integrations as rollout dependencies.
- If a supported external installer permits a destination hint, pass the selected target and verify
  the resulting installation. If it installs elsewhere, show the actual path and require the player
  to accept that result or retry; do not silently claim the original selection was honored.
- For integrations without controllable output, explain the separate build location before setup.
  They cannot satisfy the “all installed content here” guarantee until their integration is updated.
- Leave existing installations and user-selected prebuilt folders in place. Updates must retain
  their current path behavior; profile migrations must not silently relocate or rebuild them.

### 4. Verify the complete lifecycle

- Persist the exact game folder in `Installed.path` and use it for launch, setup, update, folder
  opening and uninstall. Keep pending-install metadata separate from successfully installed state.
- Audit Move, missing-drive recovery and uninstall for assumptions that every game's parent is a
  registered library. Managed output beneath the install moves with it; external source files and
  user-owned prebuilt folders retain their existing ownership rules.
- Verify input delivery under the chosen root, cross-volume copy behavior, and setup after an
  update. Preserve the exclusion of managed keys from update backups and cloud/save snapshots.
- Keep existing library records readable without migration. Do not move existing games merely
  because a destination picker is introduced.

### 5. Documentation and release notes

- Update `docs/FAQ.md` with library-parent versus exact-game-folder examples and the boundaries
  around original inputs, saves, shared files and external installers.
- Update `docs/DEVELOPERS.md` with a small example showing an actual supported builder using
  `{install}` consistently for preparation and launch. Update `docs/SETUP-INPUTS.md` if delivery
  behavior changes; otherwise preserve its named-input and shared-key rules.
- This design does not require a new `recomplauncher.json` field or feature. If implementation
  changes manifest semantics or adds one, update `docs/recomplauncher.schema.json`, the matching
  developer guide/example and the patch draft's **For the website (SDK guide)** line together,
  as required by `AGENTS.md`. Run schema parity tests. Keep website handoff text out of player notes.
- Add player-facing patch notes once implemented, describing folder selection and exactly which
  preparation flows honor it. Do not advertise unsupported external builders as covered.

## Acceptance checks

- A new Easy Install can target `E:\Games\My Recomp` or a Linux/macOS path containing spaces
  and Unicode. The release and supported generated content land below that exact path without
  an extra game-name folder. Desktop and Big Picture produce the same result.
- Picking another build or an older release, pausing/resuming, restarting and retrying all preserve
  the chosen target and release. Cancel starts no download and touches no game files.
- Updating an existing installation uses its saved location even if the default library changes.
  Existing external builds remain usable in their current locations.
- Invalid, occupied, overlapping, inaccessible and disconnected destinations fail before content
  is written. Symlink/junction aliases cannot bypass overlap checks. Concurrent requests cannot
  install into the same destination. Disk-full failures retain a recoverable, owned partial install.
- Auto-Install on/off, named inputs, multi-game recomps, an output-producing prepare step and a
  recognized Windows installer all retain the destination through setup and launch.
- Open Folder, Move, uninstall and backup/cloud behavior work for exact destinations whose
  parents were never registered as libraries. Uninstall leaves original source files untouched.
- Add focused Rust tests for destination resolution/validation and pending-install recovery, plus
  behavioral JavaScript tests for request preservation and desktop/controller entry points.
  Run `cargo test --manifest-path src-tauri/Cargo.toml` and `node --test tests/*.test.cjs` after
  implementation. Manually exercise Windows, Linux/Steam Deck, Flatpak folder permissions and
  macOS, including an external drive and a builder that previously wrote outside the library.

Implementation is complete when the destination survives the whole install/setup/update flow,
and each integration's supported output location is accurately represented to the player.
