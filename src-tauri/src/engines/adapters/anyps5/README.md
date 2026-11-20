# AnyPS5 adapter

This adapter uses one generic converter for decrypted PS5 game folders on Windows and Linux
x86-64. It does not contain title-specific code, patches, addresses, build flags or recipes.
The three initial catalog entries are imported title metadata, **not verified compatibility
with the pinned release**. Games that need bespoke port work or patches remain upstream work.

## Checkpoint and host requirements

`components.json` pins AnyPS5 [v0.1.5](https://github.com/boykopovar/AnyPS5/releases/tag/v0.1.5)
at commit `62bd7dd61e2b8a7a59262c4c145edff549bae5b9`, the exact relinker for each host, and its
matching patched PRX archive. Here “patched” means upstream's NID export naming, not a game
patch. The source archive checksum is recorded for independent source review; preparation
uses the release binaries directly. All four downloaded artifacts were verified against the
publisher SHA256s. Both runtime archives were inspected and contain 407 native PRXs, platform
support libraries and third-party notices. The main upstream GPL license is bundled separately.

There is no published matching macOS runtime in this release; no Mac target is advertised.
The Linux release requires **glibc 2.43 or newer** and **GLIBCXX_3.4.31** from libstdc++.
`getconf GNU_LIBC_VERSION` checks glibc before provisioning. It does not install or update the
system C/C++ libraries. Hosts also need the upstream CPU features and a working Vulkan driver;
see the pinned [usage guide](https://github.com/boykopovar/AnyPS5/blob/62bd7dd61e2b8a7a59262c4c145edff549bae5b9/docs/user/USAGE.md).
No drivers, firmware, keys, original console libraries or frontend distributions are downloaded.

## Input and conversion

Select one original game directory containing `sce_sys/param.json`, `eboot.bin` or `eboot.elf`,
resources and bundled modules. All executables/modules must already be clean x86-64 ELF files.
SELF containers, malformed named PRX/ELF files, symlinks, previous `.guest.prx` outputs and
ambiguous module/executable layouts are rejected. This does not unwrap containers or decrypt
content. No SFO fallback silently changes the console to PS4.

The adapter checks the catalog's exact PPSA title ID, a nonempty content version and localized
title metadata. It hashes the actual main ELF for the saved game build identity. Historical
report versions are not acceptance constraints; the selected folder's actual revision is frozen.
The source tree is copied into `Game/app0`, retaining original resources and executable bytes.
ELF headers discover all module directories and additional executables. Standard `sce_module`,
`sce_modules` and `prx` directories use upstream discovery; other directories become repeated
`--module-dir` arguments and additional executables become `--executable` arguments. Filenames
are individual process arguments, including paths containing spaces.

The recipe runs the verified private relinker with `--to-intel --registry --no-patches`,
`--sce-module-path <copied-app0>`, the original ELF and the native output path; Windows adds
`--windows`. It never autoruns the game. `--no-patches` prevents an input folder's
`relinker-patches.toml` from adding undeclared game-specific changes. Upstream still owns
conversion and rejects unsupported executable contents. Its redundant backup-module handling
is retained. Import/dependency records and every emitted native file are checked before
registration; missing non-system dependencies fail preparation.

The installation is self-contained:

```text
Game/
  eboot.exe or eboot.elf
  libs/                       pinned host runtime and licenses
  app0/                       original resources and converted guest modules
  LICENSE.AnyPS5
  *.registry.json, *.needed.json
  _sd/, _sd_mem/, _sd_backup/  runtime save locations
  _trophy/
  anyps5-input.ini             input settings, created by the runtime when used
```

The launcher's existing native-executable launch starts in the executable's parent directory,
so the working directory is `Game`. This matters: upstream save paths are relative to that
directory and there is no supported save-root override. Save/recovery directories, trophies,
input settings and the optional entitlement settings file are listed separately from copied
resources, runtime files, shader caches and writable `data`/`download0` trees. Arbitrary game
usage of those writable data trees has not been verified as a save format. They move with the
installation but are excluded from save backups. Shader cache paths use `{install}`;
`ANYPS5_OPEN_URLS=never` prevents game requests from opening browsers automatically.
Generated Linux desktop shortcuts contain staging paths and are discarded. Play launches only
the converted native executable, without the converter or a self-updating frontend.

The shared `prepared` pipeline owns download caching, input signatures, cancellation, staged
publication, frozen build pins and completed-result recovery. Existing prepared games do not
consult today's catalog, rerun the converter or update their runtime. Moving the completed
installation retains its files and relative save/profile paths.

## Catalog provenance and adding metadata

`reports.json` contains original structured Game bring-up discussions from `boykopovar/AnyPS5`.
`report_checkpoint.json` records the import date, source commit resolution and comparison with
the release. All three reports used plain upstream main commit
`b11c7bf84f5d2f636ff60166cc3371bd071e24d5` on Windows, without PRs/local changes/environment
switches, and reported gameplay. That commit is an ancestor of the pinned release. This is
historical report evidence, not testing of the release binaries or Linux compatibility.
Each generated title retains the report URL, timestamps, original body SHA256, reported version,
platform and source revision. The catalog's version/build requirements intentionally stay empty.

The importer accepts only that reviewed structured report format, upstream discussion URLs,
known ancestor commits, plain main without local changes, no environment switches, and
reports within the checkpoint's cutoff. It generates metadata and cannot supply commands.
Adding another qualifying title means importing another report and artwork; it needs no adapter
code. New report formats or commit mappings need a source review before importing them. The
cutoff conservatively precedes the release date; updates must deliberately advance the metadata
checkpoint rather than silently claim new reports describe the old release.

```sh
python3 src-tauri/src/engines/adapters/anyps5/import_titles.py \
  src-tauri/src/engines/adapters/anyps5/reports.json \
  src-tauri/src/engines/adapters/anyps5/report_checkpoint.json \
  /private/tmp/anyps5-titles.candidate.json
```

Review the candidate before replacing `titles.json`. Catalog refreshes never rewrite saved
installation pins. `artwork.json` separately maps exact PPSA IDs through the
[PlayStation-Titles snapshot](https://github.com/andshrew/PlayStation-Titles/tree/aab201da1730a8373d586110188053b282befecd)
to official PlayStation product pages. Cover and background URLs returned image bytes on
2026-11-19. Artwork conveys title identity, not compatibility; original report screenshots remain
in the evidence snapshot. New artwork should retain the same title/content/concept provenance.

## Validation and updates

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked --test anyps5_engine
PYTHONDONTWRITEBYTECODE=1 python3 src-tauri/src/engines/adapters/anyps5/test_import_titles.py
```

The actual package test reads four previously downloaded files named exactly as in the manifest,
checks checksums, extracts both archives and validates native headers and notices. It executes
neither platform's binaries and was run successfully on macOS:

```sh
RL_ANYPS5_PACKAGE_DIR=/absolute/path/to/downloads cargo test \
  --manifest-path src-tauri/Cargo.toml --locked --test anyps5_engine \
  actual_release_archives_match_the_pins_and_native_layout -- --ignored --nocapture
```

On a native Windows/Linux x86-64 machine, the ignored `pinned_native_relinker_reports_its_version`
test downloads and executes only `--version`. The ignored `real_game_preparation_and_recovery`
test accepts `RL_ANYPS5_FIXTURE` pointing to JSON with a fresh disposable `root`, catalog
`title_id` and authorized decrypted `base` folder. It prepares, recovers and activates, retaining
output for inspection. It does not launch gameplay. Those native tests have not been run here.
Startup, rendering, controllers, save/reload, moved-install launch and cancellation with real
conversion remain unverified on both target platforms.

For an update, inspect the exact upstream CLI, module scanner, output ABI, save paths and host
requirements again. Download actual release bytes, verify publisher digests, inspect archives,
update all related fields in `components.json`, preserve licenses, and rerun package/native
checks. Manifest content participates in the package identity. Bump `packaging_revision` for
changed input/command/output semantics; keep adapter record format 1 compatible. Do not
silently replace retired downloads with latest, adopt another frontend's binaries, or rebuild
an existing prepared game. Follow the shared `docs/UPDATING-ENGINES.md` recovery rules.
