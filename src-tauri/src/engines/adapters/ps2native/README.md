# PS2 native recompilation adapter

This adapter uses the generic `scripts/port.sh` pipeline from
[exoteric33/ps2-native-recomp at 9b0eef16](https://github.com/exoteric33/ps2-native-recomp/tree/9b0eef16ecd5df327eb23eb5adaf844251c2f29f).
Every catalog title follows the same code. Any executable transformation is performed by the
pinned upstream pipeline; there are no title-specific build branches in RecompLauncher.
The implemented route is Apple silicon macOS with Xcode's compiler and SDK available during
Setup. Windows, Linux and Intel Mac are not advertised. Actual Mac gameplay remains unverified.

## Preparation and Play

`components.json` pins the source archive and private Python, CMake, Ninja, Bash, SDL2 and
wgpu-native artifacts and its MIT/Apache license files by exact URL, SHA256 and size. The shared package cache verifies their
bytes before extraction. Bash and static SDL are built privately. Setup copies the selected
Apple compiler, linker, compiler resources, SDK and required non-system dylibs into the game's
output, relocates dylib references, retains licenses and records all copied file hashes.
Unused SDK manual pages, documentation and recursive directory aliases pointing to their own
folder or an ancestor are omitted; ordinary framework links are preserved; every required path is checked against
the portable installation path rules. The compiler and SDK currently add approximately 1.1 GiB to each prepared installation.
Prepared games do not share a mutable compiler or runtime; downloaded component archives are
shared, while build products stay in the installation's work folder.

The disc's `SYSTEM.CNF` BOOT2 serial must match the selected catalog entry. The boot executable
must be a 32-bit little-endian MIPS ELF. Its SHA256 identifies the exact build; `VER` supplies the
revision when present, otherwise the ELF hash supplies a revision label. No BIOS, keys or console
system files are downloaded or requested. Input support is ISO only.

The resulting `Game/engine/ps2run` contains the native translated game, static SDL, wgpu and the
upstream EE/VU translators. Its dylib dependencies must be macOS system libraries. Play invokes
that binary directly with the saved launch profile. It cannot invoke `port.sh`, `setup.sh`, a
frontend updater, Python, CMake or a network download. Unseen game code may compile during Play
using the compiler frozen in `Game/engine/compiler`; this is the original engine's live
translation behavior, not an engine refresh. The wrapper selects its own SDK, resources and
linker and strips inherited compiler search paths. Required-file validation prevents launching
when a private compiler dependency is missing. Upstream's compiler selection during Setup is
checked against the frozen wrapper before accepting output.

The shared preparation contract records the exact runtime executable and required compiler/SDK
files in the saved build identity, rejects external symlinks, registers completed output for
recovery, and publishes it atomically. Catalog/launcher updates cannot retarget a prepared game.
All runtime paths use `{install}` or paths relative to the compiler wrapper, so moves preserve
its dependency closure. Save locations are `Game/userdata/cards`, `states` and `settings`.
The ISO, engine, compiler, EE/VU caches, diagnostic reports and logs are excluded from save
tracking. `PS2_KEYS` here means the upstream controller mapping file, not console keys.

## Catalog import

`import_catalog.py` reads the pinned engine README's entire "What runs" table. Compatibility
notes and evidence come only from that engine. Pinned PCSX2 `GameIndex.yaml` provides title
names, regions and serials; its compatibility settings are not imported. Exact normalized title
matches or an unbroken full title phrase can identify an edition. An explicit upstream region
is respected. When the report omits a region, USA is a metadata edition preference, not a claim
that upstream tested that serial. Ambiguous serials and unverified translations are reported
in `skipped-titles.json`; there are no manual aliases.

The snapshot currently contains 47 resolved titles and 8 unresolved titles. Artwork uses the
exact serial in the pinned xlenore cover collection and generic title/region matching against
pinned libretro screenshots, excluding demos and unofficial translations. Football Kingdom has
no matching screenshot, so its verified cover is also the hero. All 93 distinct image URLs were
verified to return image data. Each catalog record retains upstream and identity provenance.

To regenerate, create a snapshot directory containing the checksum-verified source archive's
`ps2-native-recomp-<commit>` folder and the three files listed in `catalog-sources.json`.
Download those metadata files directly from their pinned URLs; the importer verifies their
recorded byte hashes. Then run:

```sh
python3 src-tauri/src/engines/adapters/ps2native/import_catalog.py /absolute/path/to/snapshots
```

Review `catalog-candidate.json` and `skipped-titles.json`, verify newly chosen artwork URLs, and
copy the reviewed snapshots into this folder. Updating the engine also requires reviewing this
catalog from the same source commit. A new unresolved title requires better upstream/identity
metadata; it must not introduce game-specific build code into this repository.

## Validation

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test ps2native_engine
```

The Python identity, catalog and movable compiler-wrapper tests run with that Rust target.
`native_smoke.py` is an opt-in real build using a generated 288-byte MIPS ELF and ISO, with no
game data, browser, GPU or window. It disables upstream exploration so the game's generated
code window must compile at runtime. It checks cold/warm execution, then moves the complete
package into a path with spaces and Unicode and checks warm/cold/repeated execution. The
preparation source is unavailable, PATH has no host tools, and host SDK/include variables point
to nonexistent locations. This passed using the full private recipe and frozen Xcode closure.
The native binary was also checked for arm64, static SDL window support, system-only dylibs,
and internal SDK symlinks. This test does not establish rendering or commercial-game playability.

Use a JSON mapping `source`, `cmake`, `ninja`, `sdl`, `bash` and `wgpu` to already verified
extraction roots, plus `licenses` mapping manifest license names to verified files, then run the pinned private Python:

```sh
/path/to/private/python3 -I src-tauri/src/engines/adapters/ps2native/native_smoke.py \
  --components /path/to/extracted-components.json --work '/tmp/new PS2 smoke'
```

The work directory must not already exist and remains available for inspecting logs. The
ignored Rust integration test invokes the same script using `RL_PS2_COMPONENT_ROOTS` and
`RL_PS2_PRIVATE_PYTHON`. Running normal tests does not download or build the engine.
