# Updating engines and game profiles

This is the maintenance procedure for both an engine update and a game-profile update.
The adapters integrate nxnative directly on Apple silicon Mac and experimental ScarletNX on
Windows/Linux x86-64. Follow [Adding an engine](ADDING-ENGINES.md) for the plugin contract and
automatic discovery. Keep shared storage, jobs and pins independent of individual engines.

Engine manifests and game catalogs are currently compiled into RecompLauncher. Updating these
files requires rebuilding the launcher. The app downloads the exact pinned components when
needed; it does not follow an upstream branch or dynamically replace installed games.
See [the integration guide](ENGINE-INTEGRATION.md) for the player workflow and current limits.

## Where changes belong

Paths below are relative to the repository root.

| Area | Files | What to review |
| --- | --- | --- |
| Source and dependency checkpoint | `src-tauri/src/engines/adapters/nxnative/nxnative-components.json` | Exact commit, source archive, dependency URLs, checksums, extraction roots and sizes. |
| Adapter and private tools | `src-tauri/src/engines/adapters/nxnative/mod.rs`, `bootstrap_nxnative.py` | CLI arguments, prerequisites, tool versions, offline Python dependencies, native library relocation and package identity. |
| Game profiles | `src-tauri/src/engines/adapters/nxnative/catalog-nxnative.json`, `catalog.rs` | Upstream profiles and notes, title identity, console, required package/version/build ID, supported input formats. |
| Game artwork | `src-tauri/src/engines/adapters/nxnative/artwork-nxnative.json` | Exact-title-ID mapping, cover/hero URLs and metadata provenance. |
| Saved installations | `src-tauri/src/engines/model.rs`, `pending.rs`, `coordinator.rs`, `installed.rs` | Compatibility, frozen launch/save profiles, explicit pending retries, validation and recovery. |
| Progress | `adapters/nxnative/mod.rs`, `progress.rs`, `jobs.rs` in the engine directory; `src-tauri/src/lib.rs`, `ui/app.js`, `ui/tv.js` | Engine JSON event contract and both setup displays. |
| Verification | `src-tauri/tests/engine_pipeline.rs`, `engine_progress.rs`, `engines.rs`, `engine_toolchain_test.py`; `tests/engine-*.test.cjs` | Preparation, pinning, recovery, private tools, cancellation and UI behavior. |

## Preserve these behaviors

- Obtain source directly from `seedds/nxnative` and tools from their own upstream distributions.
  NX2Apple is another frontend, not an engine or tool-distribution dependency for this adapter.
- Identify source with a full commit and verify downloaded bytes before extraction/execution.
  Do not substitute `main`, `latest`, an unpinned wheel resolver, or the developer's global tools.
- Updating the launcher, engine catalog or artwork must not rebuild an existing prepared game.
  Play uses that game's recorded executable, runtime identity and launch/save profile.
- Keep the existing Easy Install/Easy Setup file-selection flow. Provision engines privately;
  additional progress information must not become an engine-selection wizard.
- Keep old saved records readable. Do not delete caches or game work directories to force an
  update, overwrite a prepared app, or rewrite saved pins to the new recommendation.
- Follow [named-input and key requirements](SETUP-INPUTS.md). An engine update is not permission
  to add new key types, console-file downloads or unsupported target platforms.

## Updating nxnative and dependencies

1. **Select and inspect an exact upstream commit.** Compare it with the manifest's current
   `upstream_revision`. Review engine changes and game-profile changes together. Record which
   fixes motivated the update and which titles/builds they affect; do not describe every new
   upstream commit as a relevant fix for every installed game.

2. **Check the upstream build contract at that commit.** In particular, inspect
   `scripts/build_ipa_from_nsp.py`, `.github/requirements/tools.txt`, `docs/frontends.md` and
   `dm/gfx/tools/dmsh_chain.lock`. Confirm the Mac builder arguments, output layout, supported
   Python/.NET versions, native tools, Xcode/SDK/Metal requirements and progress events.
   Check any new module imports and transitive libraries. Do not bypass the Metal lock with
   `--allow-other-metal`, or use damaged-input exceptions to make an acceptance test pass.

3. **Update the source entry in `nxnative-components.json`.** Set `upstream_revision` to the full
   chosen commit and use `https://codeload.github.com/seedds/nxnative/tar.gz/<commit>`. Update the
   archive name, SHA256 `digest`, byte size and extracted `root` together. Download and inspect
   the archive actually referenced by that URL; do not use a Git tree hash as an archive checksum.
   A local archive's digest can be checked with `shasum -a 256 /absolute/path/to/archive.tar.gz`.

4. **Update only dependencies the checkpoint needs.** Each artifact must retain an immutable
   version, direct download URL, verified digest and correct layout. The downloader recognizes
   64-digit SHA256 and 128-digit SHA512 digests; the Microsoft .NET SDK uses SHA512. Check official
   publisher metadata/checksums where supplied. Homebrew bottle entries also need the exact
   registry repository, archive root (including any formula revision), and byte size: registry
   downloads enforce that size. Preserve license material in the downloaded packages.

   Python wheels must match the private interpreter's ABI, architecture and supported macOS
   version. Include the full transitive wheel set: bootstrap installs offline with `--no-deps`.
   Native tool libraries must resolve entirely to system or private libraries after relocation;
   the app must work without an installed Homebrew tree or the maintainer's Python packages.
   Keep .NET available for formats that need shader translation, not only encrypted packages.

5. **Check build identity and compatibility**, using the rules below. Update the adapter and
   bootstrap only where the new contract requires it. Preserve download cancellation/timeouts,
   process ownership, private staging and verification before publishing cached packages.

6. **Refresh and review the game profiles from that same source commit**, even if the update was
   motivated by a generic engine fix. Follow the next section, then run the validation below.

## Updating the game profiles and catalog

An upstream `dm/games/<id>/game.tsv` controls how nxnative prepares a game. RecompLauncher's
`catalog-nxnative.json` snapshots that profile for discovery, input requirements and matching the
prepared title/version/build. It is different from an installed game's frozen launcher profile,
which records how to launch that already-prepared app and where it saves.

**Keep the source archive and the profile snapshot consistent.** Do not copy profiles from a
newer branch into a catalog that still provisions an older engine. New game support often needs
engine/runtime changes alongside the profile. A game-profile update from upstream therefore
also selects the corresponding source checkpoint and archive digest.

### Regenerate a candidate snapshot

From the repository root, point `RL_NXNATIVE_SOURCE` at the extracted, checksum-verified source
archive for the manifest's selected commit. The following writes a separate candidate file for
review; it does not replace the checked-in catalog or touch an installed game.

```sh
export RL_NXNATIVE_SOURCE=/absolute/path/to/verified/nxnative-source
export RL_NXNATIVE_CATALOG_OUT=/tmp/catalog-nxnative.candidate.json
python3 - <<'PY'
import json
import os
from pathlib import Path

manifest = json.loads(Path('src-tauri/src/engines/adapters/nxnative/nxnative-components.json').read_text())
revision = manifest['upstream_revision']
source = Path(os.environ['RL_NXNATIVE_SOURCE'])
output = Path(os.environ['RL_NXNATIVE_CATALOG_OUT'])
profiles = sorted(source.glob('dm/games/*/game.tsv'))
if not profiles:
    raise SystemExit('No game profiles found; check the source layout before proceeding.')
records = []
for profile in profiles:
    readme = profile.with_name('README.md')
    notes = readme.read_text(encoding='utf-8') if readme.is_file() else ''
    summary = ' '.join(line.strip() for line in notes.splitlines()
                       if line.strip() and not line.lstrip().startswith('#'))[:1000]
    evidence = readme if readme.is_file() else profile
    records.append({
        'profile': profile.read_text(encoding='utf-8'),
        'notes': summary,
        'evidence': f'https://github.com/seedds/nxnative/blob/{revision}/{evidence.relative_to(source).as_posix()}',
    })
output.write_text(json.dumps(records, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(f'{len(records)} source profiles written to {output}')
PY
```

Review the candidate against `src-tauri/src/engines/adapters/nxnative/catalog-nxnative.json`, then replace that file
with the reviewed candidate. Inspect the resulting Git diff before committing.

### Review the profile changes

- List added/removed title IDs and changed console, name, preferred `package`, version and
  `main=` build ID. Keep IDs stable; do not join titles solely by display name. A removed catalog
  entry must not remove an installed game or change its saved identity.
- Read the changed upstream READMEs as well as the TSVs. Confirm base/update requirements,
  extracted layouts, special limitations and Mac support. An upstream profile does not by itself
  establish successful gameplay; retain experimental wording and accurate compatibility notes.
- Audit the entries the **launcher parser actually imports**, not just the number of TSV files.
  The current parser requires a first-line `(Switch`/`(Switch 2` marker, a valid title ID, and a
  preferred package with a known version and hexadecimal `main=` build ID. Profiles with other
  header wording or `?` versions can be omitted. Review unexpected omissions; do not modify the
  upstream profile text or invent identities just to make a listing appear. If upstream changes
  its format, update the parser and add a regression test for the real new shape.
- Check the resulting Easy Setup inputs against what the builder accepts. Maintain explicit
  format dependencies, matching update versions and shared-key reuse. Do not relax build-ID or
  output validation to accept files from a different game revision.
- For every newly exposed title, update `artwork-nxnative.json` with title-ID-matched cover and
  hero images and source provenance. Use Nintendo/public title metadata, check region/edition
  matches, and verify the URLs return image data. Keep artwork separate from build identity.
- A correction to catalog notes, labels or artwork at the existing checkpoint can stay metadata
  only. A preparation-affecting correction needs the checkpoint/identity review below. Neither
  kind silently replaces the profile already saved with an installation.

## Checkpoint identity and existing games

| Change | Required handling |
| --- | --- |
| Upstream source or upstream game profiles | Pin the corresponding full source commit and verified archive; regenerate the catalog from it. |
| Dependency manifest or private bootstrap | Update the verified artifacts/recipe. `nxnative::pin()` derives `toolchain_sha256` from the manifest text and bootstrap script automatically. Even formatting changes to these inputs change that cache identity. |
| Launcher preparation behavior not covered by those hashes | Change `packaging_revision` in `nxnative::pin()` when build arguments, input interpretation or output production change, so unfinished jobs do not reuse a different recipe under the same identity. |
| Engine source patches | The current source is unpatched and `patch_sha256` is SHA256 of empty bytes. If introducing patches, define a reproducible application/hash procedure and update the identity; do not claim the empty digest for modified source. |
| Saved-record compatibility | `adapter_version` is currently **1**, and validation accepts only 1. It is not an engine release counter. Do not increment it for a routine update or invalidate old pins. A new contract needs explicit backward-compatible reading/dispatch and tests. |
| UI progress, notes or artwork only | Do not bump the build recipe or rebuild games just for presentation changes. |

The final prepared pin additionally records the actual Apple tool version outputs and the .NET
digest. Do not replace that saved pin with the currently recommended package when showing Versions.

Installation behavior to preserve:

1. **New installation:** uses the current checkpoint and catalog profile.
2. **Unfinished installation:** stores a snapshot of both. `pending::for_setup` adopts the
   current profile/checkpoint only on explicit Setup retry **and only when the package pin has
   changed**. Editing the catalog alone does not refresh an existing pending snapshot. If a
   preparation-affecting profile correction must be adopted on retry, give its recipe a distinct
   checkpoint rather than silently rewriting records during refresh.
3. **Completed output awaiting registration:** `.engine-prepared.json` retains its original
   pin and inputs. Recover/register it without recompiling. Keep the Move guard until registration
   finishes; do not delete the recovery record to force the newer engine.
4. **Prepared game:** retains its original game identity, package/runtime pins, executable and
   launch/save profile. Catalog refreshes, Play and ordinary launcher updates do not rebuild it.

Applying a relevant engine fix to a prepared game is not implemented in this test build. That
future action must be explicit for the selected game and replacement, preserve saves, validate a
separate result and support rollback. Updating the default checkpoint does not supply that action.

## Validate an engine or preparation-affecting profile update

Run from the repository root on a Mac with the selected checkpoint's supported Xcode/Metal tools:

```sh
node --test tests/*.test.cjs
cargo test --manifest-path src-tauri/Cargo.toml --locked
PYTHONDONTWRITEBYTECODE=1 python3 src-tauri/tests/engine_toolchain_test.py
```

Add focused regression tests for changed profile formats, CLI/output contracts, dependencies or
progress events. The current progress contract includes `stage`, `step`, `progress`, `stage_end`,
`k`, `n`, `frac`, `done`, `total`, `unit`, `est`, `eta` and `elapsed`. ETA is for the current substep;
elapsed is for the stage. Preserve null/unknown values, reset estimates between steps, and check
both desktop and Big Picture. Do not calculate overall progress from `elapsed / (elapsed + eta)`.

### Fresh preparation and recovery

Use a **new, disposable fixture root with an empty component cache** for each candidate checkpoint.
Reusing a completed fixture only exercises recovery; it can pass without running the updated
engine. Use the player's authorized game files and matching update, never committed game data or
key contents. Keep the fixture JSON outside the repository, with this shape:

```json
{
  "root": "/private/tmp/rl-engine-candidate-unique",
  "title_id": "0100633007D48000",
  "base": "/absolute/path/to/base.nsp",
  "update": "/absolute/path/to/matching-update.nsp",
  "keys": "/absolute/path/to/prod.keys"
}
```

Choose a title/build supported by the candidate catalog. `update` and `keys` are optional where
the selected input format/profile does not need them. An optional `archive` field can point to
the exact verified source archive; dependencies still need independent provisioning.

```sh
RL_ENGINE_FIXTURE=/absolute/path/to/fixture.json cargo test --manifest-path src-tauri/Cargo.toml --test engine_pipeline real_nsp_preparation --locked -- --ignored --nocapture
```

Verify signature, exact title/version/main build ID and native launch target. Exercise at least
one affected game/profile where the necessary files are available; list formats or games not
tested. Repeat the same fixture to verify completed-output recovery without rebuilding. Check
that an older prepared record still loads, launches its original executable and retains its pin,
and that cancellation/retry and pending checkpoint migration keep their existing protections.

For a startup/play test, use supported engine save/temp/system overrides with a disposable data
location and disable save sync. Do not repurpose `HOME` or overwrite the player's normal nxnative
saves. The preparation fixture isolates launcher data/build output; launching the resulting app
normally still uses nxnative's per-title save location. Report preparation, startup and gameplay
evidence separately.

Replay a candidate preparation log to check progress without another game build:

```sh
RL_ENGINE_PROGRESS_LOG=/absolute/path/to/preparation.log cargo test --manifest-path src-tauri/Cargo.toml --test engine_progress recorded_build_progress --locked -- --ignored --nocapture
```

The replay test expects at least one positive ETA and multiple completed stages; use a real,
measured build log rather than a short cached run that legitimately reports no ETA.

### Rebuild the launcher and record the result

```sh
npm run build -- --debug --bundles app --config src-tauri/tauri.switch-test.conf.json -- --locked
codesign --verify --deep --strict 'src-tauri/target/debug/bundle/macos/RecompLauncher Switch Test.app'
```

Check the separate test app's Discover artwork, selected-file flow, progress in both modes and
Versions display. Keep the source/dependency checkpoint, profile snapshot, artwork changes and
compatibility adjustments together in the reviewed change. Update `ENGINE-INTEGRATION.md` when
its pinned source link, prerequisites, supported formats or validation claims change.

Record the old/new checkpoint, dependency changes, profile additions/removals/version changes,
affected titles, test commands/results and untested cases. Clearly state that existing prepared
games retain their pins. For a failed candidate, return the default recommendation to the previous
verified checkpoint; do not rewrite saved games or remove their caches as a rollback mechanism.

Metadata-only documentation/artwork corrections do not require rebuilding a game. Verify the
changed links/metadata and relevant catalog/UI behavior instead. Publication is separate from
validation: follow the user's current local/fork/upstream instructions; this guide does not
authorize pushing a branch, opening an issue/PR or updating somebody else's engine repository.


## Updating ScarletNX and its title metadata

The adapter, output validator, component manifest and title metadata live together in
`src-tauri/src/engines/adapters/scarletnx/`. Update both the adapter and its catalog deliberately:

1. Select an exact source commit and its release in `scarletnx/nightly`. Verify each platform's
   actual downloaded bytes against the publisher's SHA256 metadata. Update the full source
   revision, release tag, URL, digest, size and filename in `scarletnx-components.json` together.
   Inspect the Windows ZIP layout/DLLs/licenses and Linux AppImage rather than assuming continuity.
2. At that same commit, inspect `docs/user/usage.md`, the driver extraction/recompilation code,
   runtime startup, portable folders, ABI stamp and deployed executable/data layout. Setup uses
   extraction followed by `-r --extract-container`; Play must keep using the game's own runtime
   with `--game=...`, bypassing automatic recompilation and runtime refresh.
3. Keep game data self-contained. Revalidate title/version/build from NPDM, NSO and NACP; check
   compiled code, completion stamp, exact runtime and Windows libraries. Recheck portable save,
   user and settings paths and remove stale source/absolute build-folder references before moving.
4. Review `titles.json` independently. It is title/name/artwork metadata copied from identified
   nxnative profiles, **not** ScarletNX's compatibility list. ScarletNX currently publishes no
   named supported-game catalog. Keep every entry Switch 1-only and explicitly unverified. Do
   not inherit nxnative version/build requirements or compatibility claims. Retain provenance and
   exact title-ID artwork matches; use ScarletNX's own evidence if it later publishes a catalog.
5. The manifest text contributes to `toolchain_sha256`. Change `packaging_revision` for changed
   CLI/input/output preparation behavior not covered by that hash. Preserve prepared pins and
   completed-result recovery; a catalog refresh is not a game rebuild.
6. Run `scarlet_engine` contract tests and the full suites. On Windows/Linux run the ignored
   `pinned_desktop_package_starts_its_cli` smoke test. Check real preparation, Play, controllers,
   saving/reloading, move, cancellation and retry on each platform before claiming compatibility.
   Mac contract tests cannot establish Windows/Linux gameplay.

The nightly publisher retains only the newest 30 releases. Checksums prevent silent substitution
but cannot keep a deleted download available. Retained, authorized hosting or a durable upstream
release is a release-readiness requirement. Never silently resolve a missing pin to `latest`;
existing prepared games retain their own runtime and are unaffected by download retirement.
