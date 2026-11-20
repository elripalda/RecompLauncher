# PS4Recomp adapter

This adapter runs the generic upstream `ps4-recomp build` pipeline on Apple silicon Mac. Select
an extracted, decrypted game folder containing `eboot.bin` and `sce_sys/param.sfo` through normal
Easy Setup. The ELF must be x86-64, and the SFO title ID must match the selected game/region.
The actual application version and executable SHA256 become the installation identity.
Encrypted SELF/PKG conversion, separate updates and DLC are not implemented.

## Checkpoint and private tools

`components.json` pins the `nightly-20261115` engine archive and source revision
`354fdaeeb02c0ad6258aa7f87c677102e2ccbb2d`, including archive hashes and extraction layouts.
Xcode's compiler and macOS SDK are prerequisites. CMake, FreeType and FFmpeg are downloaded from
their own pinned upstream sources and built privately. No Homebrew installation, global
pkg-config or console system-file downloads are used. FFmpeg uses static software decoders;
host codec autodetection and networking are disabled. FreeType is a rendering library, not a
console font download. The engine source is not patched.

The manifest and `pipeline.rs` identify the recipe; the completed game records the actual Apple
compiler/SDK receipt. Changes to preparation behavior in `mod.rs` also require a distinct
`packaging_revision`. Keep `adapter_version` compatible with existing format-1 saved records.
Tools are reused within the matching pending work directory. Downloaded immutable archives are
shared; compiled tools are not yet shared between different game installations.

## Preparation and Play

Setup copies the selected folder into private staging, provisions tools, and invokes the engine
with `--standalone --json`, without autorun or title-specific switches. The CLI provides measured
work counts; no overall ETA is invented. Setup verifies the ARM64 bundle entry, system-only native
dependencies, copied resources, compiled guest image and app signature before registration.
Upstream and dependency notices are retained under `Game/licenses`.

Play starts the recorded app executable directly. `PS4_APP_DIR` points at that installation's
bundle resources; `PS4RECOMP_DATA_DIR` and `PS4_SAVEDATA_DIR` isolate its writable state. Backups
include only `Game/state/saves` and `Game/state/settings`; temporary/downloaded content is
excluded. Source, runtime and game assets do not become saves. Moving the prepared game keeps
expandable paths; neither moving nor Play invokes the compiler.

## Catalog and maintenance

The catalog comes from the compatibility table in the exact engine source README. Only upstream
rows reporting playable or in-game status are imported, with the original limitations retained.
These are upstream reports, not launcher gameplay certification. No title-specific code, patches,
compiler flags or harnesses are stored in this adapter.

`refresh_catalog.py` joins title IDs against the pinned PlayStation-Titles metadata in the
manifest, then obtains official PlayStation cover/background images. Artwork is independent of
runtime compatibility. Generate a candidate from the checksum-verified engine source and the
exact metadata revision, then review the diff:

```sh
python3 src-tauri/src/engines/adapters/ps4recomp/refresh_catalog.py \
  --source /absolute/path/to/verified/PS4Recomp-source \
  --title-data /absolute/path/to/pinned/PS4_Titles.json \
  --output /tmp/ps4-titles.candidate.json
```

When updating the engine, inspect its CLI, SFO/ELF handling, static dependency requirements,
resource-copy rules, save/settings paths and JSON events. Reverify every artifact and regenerate
the catalog from the same checkpoint. Run the tests and native smoke before publishing a new
recommendation. Existing completed games/results retain their original pins.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test ps4recomp_engine --locked
python3 -m unittest discover -s src-tauri/src/engines/adapters/ps4recomp -p 'test_*.py'
RL_PS4_SMOKE_ROOT=/absolute/path/to/isolated/smoke \
  cargo test --manifest-path src-tauri/Cargo.toml --test ps4recomp_engine \
  native_synthetic_folder --locked -- --ignored --nocapture
```

The opt-in smoke downloads/builds the real pinned tools and converts a copyright-free synthetic
ELF. It validates registration and relocation; it does not establish commercial-game rendering,
audio, controller or save compatibility. Use a fresh smoke root for a fresh run. Windows/Linux
are not advertised by this adapter; they need their own verified upstream runtime route.
