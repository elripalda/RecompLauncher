# Docs and schema stay in step with the launcher

Any change that adds or changes something a developer can write in recomplauncher.json (a field, a
value, a shared type, a launcher feature in FEATURES) must, in the same commit:

- update `docs/recomplauncher.schema.json` (recomplauncher.com copies it from main);
- update the matching docs file (`docs/DEVELOPERS.md`, `docs/SETUP-INPUTS.md` or the one that covers
  it), with a small example;
- add a line to the patch notes draft under "For the website (SDK guide)", saying in one or two
  plain sentences what the SDK guide on recomplauncher.com should add or change. That line is for
  Ruben to pass to the website chat, not for players: leave it out of the notes players see.

`cargo test` fails when they drift (devtools.rs: the schema parity test).

# Engine and game-profile updates

Before adding an engine, read [docs/ADDING-ENGINES.md](docs/ADDING-ENGINES.md). Keep platform and
game-system support, catalog/input rules, provisioning and preparation in the adapter folder.
Use automatic discovery and the shared contract; do not add engine-name branches to shared flows.

Before updating an engine checkpoint, build dependency, adapter, or game-profile catalog, read
[docs/UPDATING-ENGINES.md](docs/UPDATING-ENGINES.md). It covers **both engine updates and game-profile
updates**, including their source relationship, artwork, compatibility and required validation.

Keep nxnative source and game profiles tied to the same verified upstream commit. Integrate
nxnative directly; do not obtain the engine or its tools from NX2Apple's frontend distribution.
Preserve existing prepared games and their saved pins: an engine or profile refresh must never
silently rebuild or retarget them. Pending retries and completed-result recovery have different
rules; follow the guide. Keep the normal file-selection workflow and both setup views consistent.

# Named Easy Setup input scope

When changing named setup inputs or shared keys, read [docs/SETUP-INPUTS.md](docs/SETUP-INPUTS.md).
The supported shared types are closed: `switch.prod-keys`, `switch.title-keys`, `wiiu.common-key`,
and `nvidia.dlss` (the maintainer's explicit decision for NVIDIA DLSS only: the one file the
launcher downloads itself, from NVIDIA). Do not add BIOS, firmware, fonts, system libraries,
emulator configuration, Switch 2 key support or any other automatic download without a new
explicit request. Research findings are not permission to expand implementation.

Multidisc/DLC support collects named paths; content hashing and verified-disc-set claims are deferred.
Do not infer key requirements from an NSP/XCI extension or console: only explicit manifest format
dependencies activate keys. Compatible stored keys are reused automatically. Keep desktop and
Big Picture behavior consistent and prevent managed key copies from entering backups/cloud saves.

## Conventions

- Follow existing conventions/patterns if present unless told otherwise.
- Comments explain why, in plain words, not what the code already says. They're written for the next person (and agent) who reads the code.
- When possible, avoid multi line expressions, nested loops, and nested conditionals. Favor early returns and breaking out of loops.
- Terseness is per line, not per statement: prefer an extra named variable over an expression that spans lines.
- Keep code simple, readable, and self documenting; avoid cleverness. Favor clarity over performance unless the code is in a hot path.
- When writing tests, prefer simple, explicit tests over complex fixtures or mocks. Test one thing per test function.
