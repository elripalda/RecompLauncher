# Named Easy Setup input scope

When changing named setup inputs or shared keys, read [docs/SETUP-INPUTS.md](docs/SETUP-INPUTS.md).
The supported shared types are closed: `switch.prod-keys`, `switch.title-keys`, `wiiu.common-key`.
Do not add BIOS, firmware, fonts, system libraries, emulator configuration or Switch 2 key support
without a new explicit request. Research findings are not permission to expand implementation.

Multidisc/DLC support collects named paths; content hashing and verified-disc-set claims are deferred.
Do not infer key requirements from an NSP/XCI extension or console: only explicit manifest format
dependencies activate keys. Compatible stored keys are reused automatically. Keep desktop and
Big Picture behavior consistent and prevent managed key copies from entering backups/cloud saves.
