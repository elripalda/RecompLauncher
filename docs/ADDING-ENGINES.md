# Adding an engine adapter

Engines behave as compiled plugins. Each adapter owns its target support, game systems, catalog,
input rules, package recipe, preparation, validation and activation. Shared launcher code owns
Discover, Easy Install/Easy Setup, Versions, process jobs, package verification and saved builds.
A player selects game files in the existing flow; the launcher provisions the engine behind the
scenes. Engine selection belongs in Versions, not a new installation wizard.

## Discovery and layout

```
src-tauri/src/engines/
  registry.rs                    # Adapter contract and runtime target selection
  catalog.rs                     # Merge adapter catalogs into Discover
  pending.rs, installed.rs       # Pending records and frozen prepared builds
  coordinator.rs, jobs.rs        # Dispatch, file helpers and owned processes
  prepared.rs                    # Generic staging, frozen records and atomic activation
  packages.rs                    # Verified immutable package cache
  adapters/
    nxnative/
      mod.rs                     # ENGINE implements Adapter
      catalog.rs, pipeline.rs
      nxnative-components.json, catalog-nxnative.json, artwork-nxnative.json
      bootstrap_nxnative.py, watch_parent.py
    scarletnx/
      mod.rs, output.rs
      scarletnx-components.json, titles.json
```

`src-tauri/engine_registry.rs`, called by `build.rs`, discovers adapter directories containing
`mod.rs`, sorts them, and generates modules and `ADAPTERS` into Cargo's output directory. There is
no hand-maintained list of engine names. At runtime `registry::available(Target::current())`
selects adapters by their declared targets; the saved engine ID selects an unfinished game's
adapter. Prepared games launch from their saved profile without consulting the current adapter.

Keep an engine in **one folder**, including when it supports several host platforms. Dividing
adapters into Mac/Windows/Linux directories would duplicate a cross-platform engine. Declare
platforms in the contract instead. Adding an adapter does not require editing the launcher,
registry, dispatcher, or existing adapters. Add its tests and documentation alongside the new
folder. A new kind of shared capability may still require a deliberate contract extension.

Discovery happens when compiling the launcher. This is not arbitrary runtime loading of native
libraries, a user-writable plugin directory, or a live updater for executable adapter code.
Pinned engine packages download on demand. Adding/changing adapter Rust code or bundled catalog
metadata requires a launcher build. Authenticated live catalog/package recommendations remain
future work; they must never silently change a prepared game's pin.

## Required contract

Implement `engines::registry::Adapter` and export `pub static ENGINE: Engine` from `mod.rs`.
The folder name must be a valid lower-case Rust identifier; use a stable alphanumeric/hyphen
engine ID in saved records. IDs must be unique across adapters.

| Member | Adapter responsibility |
| --- | --- |
| `id`, `repo`, `status` | Stable engine identity, upstream source and honest compatibility label. |
| `targets` | Supported host OS/architecture combinations, not the console being recompiled. |
| `systems` | Supported game-system IDs, such as `switch`, `switch2`, `gc`, `wii` or `ps2`. Declare only implemented routes. |
| `games` | Normalized title metadata and engine-specific profile requirements, with source evidence. Unsupported systems must not appear. |
| `artwork` | Exact-title cover/hero metadata. Artwork is independent of build compatibility. |
| `profile` | Existing named Easy Setup inputs with explicit format/key dependencies. |
| `input_signature_version` | New generic preparations opt into `TreeV1`; older adapters/records retain the default legacy behavior. |
| `pin` | Exact immutable source/package/dependency/recipe identity for the selected target. |
| `preflight` | Required host capabilities, with an actionable error before preparation. |
| `output_present` | Detect completed or interrupted output so a new checkpoint cannot overwrite it. |
| `prepare` | Provision privately, resolve inputs, prepare in staging, validate, and persist a recoverable result. Return an `Installed` result without publishing it. |
| `activate` | Recheck ownership/cancellation and publish a completed result atomically. Recover either side of an interrupted rename without rebuilding. |

System IDs are data. Shared identity records retain the original `console` and `title_id` JSON
field names for compatibility, but `console` can hold another system ID and `title_id` is an
opaque game ID. Each adapter validates its own identity format: Switch title IDs and NSO/NPDM
rules belong in Switch adapters. Adding a non-Switch adapter does not require a new shared enum
variant or pretending its game IDs are Switch IDs. Use stable lower-case system IDs matching the
launcher catalog where available. New systems may need presentation metadata in the UI's system
catalog; that does not require engine-specific installation or launch branches.

`EngineGame` is the catalog/pending snapshot. `package`, `version` and `build_id` describe an
adapter's selected profile; leave unknown revision fields empty in experimental discovery metadata
and identify the actual revision during preparation. Do not invent compatibility evidence.
The first saved format omitted `engine` and is interpreted as nxnative for backward compatibility;
that compatibility default is not an engine selector.

## Preparation and saved-game invariants

### Games are upstream data, not launcher code

A new game must work through the engine's existing generic pipeline or through profiles shipped
by that engine. Catalog importers may normalize upstream title IDs, revisions, evidence and
artwork. They must not contain a table of title-specific compiler switches, patches, generated
code, harnesses or launch exceptions. If a game needs that work, contribute it to the engine
first. If an engine requires a separate developer port for each game, defer its integration
until it supplies a generic route or a centrally maintained set of ready profiles.

Keep source, profiles and compatibility evidence tied to their verified checkpoints. A list of
disc metadata is not a supported-game catalog. When reports refer to a different source revision
than the pinned release, label them explicitly as unverified for that release. Ambiguous title
matches are omitted and reported by the importer; do not fix them with hand-written game rules.

### Reuse the generic preparation transaction

New adapters can delegate `prepare`, `output_present` and `activate` to `engines::prepared`.
The adapter's build callback receives a `Context` with resolved paths, the saved pending record,
an installation-local work/output directory, cancellation, verified package helpers and process
execution. It returns an `Output` descriptor:

```rust,ignore
prepared::Output {
    identity: actual_game_identity,
    start: frozen_native_launch,
    saves: vec!["state/saves".into()],
    files: vec!["runtime".into(), "content".into()],
    required_files: vec!["runtime/receipt.json".into(), "content/main.bin".into()],
}
```

All descriptor paths, including `start.program`, are relative to `Context.output`. Launch
arguments and environment values use `{install}/Game/...`; the helper prefixes the native
program, required files and save paths when freezing the record. The helper verifies identity,
native output and path containment, hashes the program and required files into the runtime
identity, records a durable completed result, and publishes it atomically into `Game/`. It
preserves the current player's preferences on recovery. Recovery precedes fresh build-tool
checks, so a completed result does not require reinstalling today's compiler.

New `TreeV1` pending records also snapshot the input profile. Use `Pending::profile()` or
`pending::profile()` when resolving an existing installation, so changes to labels, accepted
formats or current adapter code cannot strand its completed result. Only an eligible explicit
retry with a changed checkpoint replaces that snapshot. Nested input metadata checks and runtime
hash verification are cancellable work and must run outside global library/setup locks.

The adapter remains responsible for validating its actual output format, runtime dependency
closure, source identity, mutable data paths and upstream command behavior. Include a toolchain
receipt in `required_files` when the runtime depends on locally selected tools. List required
runtime files individually, but list owned parent roots in `files` when practical; the helper
removes redundant descendants from backup exclusions. Saves must not overlap owned content.

`jobs::CommandSpec` accepts argument arrays, explicit environment values and `remove_env`.
Declare inherited variables to remove inside the adapter. The shared process runner must not
know a particular engine's environment names. It removes those variables before applying the
adapter's explicit values. Do not interpolate paths or metadata into shell command strings.

### Invariants

- Download only exact, checksum-verified packages from the engine/tools' own sources. Keep
  licenses. Never fetch another frontend as the engine or run catalog-supplied shell commands.
- Reuse `packages`, `jobs`, named-input resolution, and structured `Progress`. Do not install
  global tools, overwrite shared caches, expose key contents in logs, or request a new wizard.
- Use an installation-local work directory keyed by package identity and input signature. Check
  that selected files still match before registration. Check exit status and actual output;
  a success message or 100% progress alone is insufficient.
- `BuildPin` freezes the game identity, engine/runtime packages, components, launch/save profile,
  native executable and required files. It must not contain preparation actions. Play must not
  call a driver path that upgrades the runtime or recompiles on an ABI change.
- List generated runtime/game-data files in `Installed.files` so backups cannot treat source
  content as saves. Keep actual saves/settings separate, with explicit `{install}` or other
  supported path templates. Never include keys, extracted game content or compiler caches.
- Keep paths relative/expandable so moving a completed installation works. Reject output links
  escaping the staged tree and missing/wrong-target native files. Keep completed results when
  registration fails; do not rebuild just because the library write was interrupted.
- Preserve current player preferences during activation. An unregistered result belongs to its
  original installation generation and inputs. Keep Move disabled until registration finishes.
- Refreshing Discover, updating the launcher, or adding another adapter must not alter prepared
  games. Pending Setup retry can adopt a new recommendation only under the documented rules.
  Applying a relevant fix to a prepared game requires a separate explicit action; no such action
  is implemented yet. There are no automatic game rebuilds.

Progress is shared UI in desktop and Big Picture. Report real measurements only when an engine
supplies them; otherwise show activity and stage messages. Do not fabricate a percentage or ETA.

## Contribution checks

Add contract tests covering declared targets/systems, catalog provenance/artwork, wrong-target
and wrong-game rejection, command arguments with spaces, verified packages, output validation,
per-game pins, cancellation, interrupted activation, moves, save inclusion and data/key exclusion.
Use synthetic metadata/native headers for contract tests; never commit games or keys. Add an
opt-in native package smoke test, plus an opt-in preparation fixture where practical. Clearly
separate these checks from actual gameplay, controller and save testing on each target.

Run the existing Rust and UI suites as well as the new tests. Keep the Mac adapter's original
checkpoint and prepared records readable. The auto-discovery regression test demonstrates a new
folder being registered without a central code edit; the non-Switch identity test protects the
generic system contract. Follow [engine and profile maintenance](UPDATING-ENGINES.md) for updates
and [setup-input scope](SETUP-INPUTS.md) for inputs/shared keys. An adapter is not permission to
add automatic firmware/BIOS/font downloads or new key types.
