# ModernGekko adapter

This is an experimental generic GameCube/Wii preparation route for Apple silicon
macOS. Every catalog entry remains **unverified**. A successful engine or synthetic
module build does not establish that a commercial game boots or plays correctly.
Windows/Linux and Intel Macs are not advertised by this adapter.

## Coherent source checkpoint

The baseline is ModernGekko-Template `c2553a99584b2045dbe46a70e1570994dae8beef`:
ModernGekko `edd460adad0e82b56909af1d793642c83b6d31c8`, its RecompCore gitlink
`10f7ac4fe48f74485fbc161ebf4d67242e11e671`, and nested DolRecomp gitlink
`1bec3554ecc4817cf78319ca3d8a0669477f29fa`. Newer independent repository heads
must not be mixed into that baseline without another integration audit.
`components.json` records the 26 source archives and private CMake, Ninja and
Python packages: direct upstream URLs, exact roots, lengths and SHA256 digests.
All source archives were downloaded and checked. Upstream retains an outdated
RecompCore revision string in its module manifest; our source receipt records the
actual gitlink. Module acceptance checks the disc ID and original DOL hash.

Setup requires Xcode's selected macOS compiler and SDK (`xcrun`). Everything else
is downloaded privately and verified by the shared package layer. CMake uses
bundled static libraries, the C recompiler backend and native Metal. Vulkan,
MoltenVK fetching, LLVM, Rust, tests and Git discovery are disabled. An empty
pkg-config implementation handles optional platform detection. `HAVE_PIPE2=OFF`
avoids an upstream curl feature-detection failure with SDK27 and an older host.
`private-python.cmake` is the only source override: it makes nested module builds
use Setup's pinned interpreter rather than a globally installed Python framework.
Its SHA256 is included in the package pin. It has no game-specific behavior.

Downloads are shared; compiled tools are reused on retries of the same pending
installation. Each new game currently builds its own tools. The native smoke build
used about 163 MB of build output, excluding source archives and game modules.
This adapter does not install tools globally or require Homebrew.

## Preparation and frozen output

The base input is a disc ISO or an extracted folder containing `sys/boot.bin`,
`sys/main.dol` and `files/`. Disc images use ModernGekko's own `--extract` operation
with a private XDG data directory. The upstream extractor requires its
`MODERNGEKKO_REQUIRED_DISC_ID` option; Setup supplies the selected metadata ID
to the same generic build recipe for every game. The runner inherits that check. The adapter never calls DolRecomp `--setup`
(which downloads extra tools) or the template's WIT-dependent Makefile.

The disc magic, six-character identity and disc revision come from boot metadata;
the selected catalog entry must match. The DOL's sections and executable entry
are validated, and its SHA256 identifies the prepared executable revision.
Only disc 1 is currently accepted. Wii save scoping also validates the partition
TMD's game title against the disc. No keys, BIOS or firmware input is introduced.

`moderngekko-port build --backend c --toolchain clang` generates the native game
module with upstream-owned game settings. No game-specific port code, aliases,
patches or launcher branches are maintained here. The module descriptor is checked
with `moderngekko-module-info`. Both the arm64 runner and dylib must depend only
on macOS system libraries; unexpected external dependencies fail Setup.

Published `Game/` contains `Disc/` and `Runtime/` (runner, module, `Sys/`, licenses,
source/toolchain receipt and upstream module manifest). Play executes the recorded
runner with exact `--game`, `--module` and `--user-dir` paths. It never invokes a
compiler, package manager, frontend build driver or interpreter fallback. Existing
prepared pins use shared recovery/activation and do not track future checkpoints.

Player data lives in `Game/User/`. Save rules include configuration (including runtime-generated SYSCONF), states,
GameCube GCI cards and raw cards/SRAM, or the selected Wii title's `data/` directory.
Disc files, Wii tickets/title content and engine components are owned files,
excluded from save scopes. Upstream supplies default keyboard mappings; controller
configuration is preserved. No additional controller UI or per-game mapping is
implemented here.

## Runtime assets and licenses

`Sys/` is copied from this exact RecompCore source tree, as upstream's runner build
specifies. There is no separate download of console dumps, keys, system firmware,
fonts or emulator configuration. Source-owned runtime assets include:

- `GC/font_western.bin` and `font_japanese.bin`: generated from Droid Sans;
  `GC/font-licenses.txt` contains the Apache2 license and generator provenance.
- `GC/dsp_rom.bin` and `dsp_coef.bin`: Dolphin's free DSP replacement v0.4,
  with source/generator in `docs/DSP/free_dsp_rom/`. After endian conversion their
  Adler32 hashes are `e789b5a5` and `a4a575f5`, matching the documented replacement.
- `Resources/OSD_Font.ttf`: VeraMono with its accompanying copyright notice.
- `Wii/shared2/wc24/`: upstream mailbox/configuration templates, not a firmware,
  IOS or NAND-title distribution. No WAD, keys or certificates are supplied.

Original source license/copyright/notice/provenance files are retained under
`Runtime/Licenses`; the source/package caches retain the complete source and tool
notices. Ninja's binary zip omits its notice, so `Ninja-LICENSE` retains the exact
COPYING text from the release commit recorded in `components.json`.

## Reproducible catalog import

The default catalog matches names in the pinned ModernGekko README's Hall of Fame
against exact disc IDs/names in the same runtime's GameSettings metadata. Normalized
text matching is generic. It does not translate abbreviations or infer compatibility.
The current four matching titles have verified GameTDB cover URLs and provenance;
the same cover is the hero fallback. Abbreviated `007 AUF` and
`Kirby Wii` names are reported unmatched rather than mapped by an alias.

```sh
python3 import_catalog.py /path/to/RecompCore \
  --revision 10f7ac4fe48f74485fbc161ebf4d67242e11e671 \
  --readme /path/to/ModernGekko/README.md \
  --artwork artwork.json --output titles.json
python3 -m unittest discover -s . -p 'test_*.py'
```

An optional `--selection ids.json` accepts a data-only list of exact upstream disc
IDs, rejects missing metadata and uses the same pipeline. Adding games requires
metadata/artwork and upstream evidence, never adapter code. An ID existing in
GameSettings is identification evidence, not a successful gameplay report.
Refresh each immutable source artifact by following its exact parent gitlink,
inspect newly added dependencies, download its archive and recalculate SHA256/size
and root. Repeat the native source/module smoke and catalog checks before updating
pins. Follow `docs/UPDATING-ENGINES.md`; never replace old prepared-game pins.

## Verification and limits

The pinned source runtime, extraction frontend, DolRecomp, port driver and module
inspector built on macOS arm64 using private CMake/Ninja plus Xcode. A synthetic,
copyright-free DOL compiled into an arm64 dylib using the pinned private Python.
The relocated module passed ABI/disc-ID inspection and loaded in the direct
headless runner. The synthetic fixture exited before native instructions ran;
this checks loading, not CPU execution. Runtime linkage was checked
for system-only dependencies. A generated copyright-free GameCube ISO exercises image extraction; no commercial
game image was available, so commercial boot/gameplay and real controller/save behavior remain
unverified. Synthetic metadata/save-scope/cancellation tests cover adapter rules;
shared prepared-engine tests cover publication, recovery and immutable Play behavior.
