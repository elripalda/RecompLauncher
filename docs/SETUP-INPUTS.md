# Named Easy Setup inputs and shared keys

This extension to format 1 lets a project ask for multiple discs, required updates and optional
DLC as separate named inputs. Easy Setup displays compact rows grouped into required files,
shared keys, and optional content. Four discs and three DLC files use seven rows, not seven
wizard pages. The original disc artwork and two-column setup layout are preserved. Optional DLC
starts collapsed with an added-file count; expanding it exposes the individual slots. On smaller
windows the artwork sits above the scrolling file list. Preparation starts explicitly after every
required input is selected.

Existing `game`, `game.update`, `start`, and `prepare` profiles remain supported. Named inputs
may accompany a legacy `game` declaration, or replace it entirely. When replacing it, use
`{input:id}` instead of `{game}`. Names refer to input IDs, not shared-resource names.

## Scope constraint for maintainers and coding agents

**Do not expand this feature into a BIOS/firmware manager.** The agreed implementation supports
four shared types: `switch.prod-keys`, `switch.title-keys`, `wiiu.common-key`, and `nvidia.dlss`.
The runtime and JSON schema use a closed allowlist. BIOS images, firmware bundles, fonts, system
libraries, emulator profiles, Switch 2 key compatibility and provider/cloud sync are out of scope.
Adding any shared type, or any other automatic download, requires a separate explicit
user/maintainer decision; research notes are not authorization to implement their other findings.

`nvidia.dlss` is that decision, made by the maintainer for NVIDIA DLSS only (Beta 37): the one
shared type the launcher downloads itself, from NVIDIA's own GitHub. No other NVIDIA files
(Streamline, frame generation, ray reconstruction).

**Multidisc content hashing is deferred.** The launcher checks that the declared files/folders
exist and match their accepted format, and refuses assigning the same path to two named slots.
It does not identify a renamed disc, prove that Disc 2 is really Disc 2, verify regions/revisions,
or mark a multidisc set as a verified dump. Labels come from the manifest and the player's choices.
The project's prepare program must perform its own content validation. Existing single-copy
CRC32 checking remains unchanged.

## Conditional keys: never inferred from NSP/XCI

```json
{
  "format": 1,
  "inputs": [
    {
      "id": "base",
      "label": "Your game",
      "formats": [
        {"type": "folder"},
        {"type": "file", "extensions": ["nsp", "xci"], "requires": ["keys"]}
      ]
    },
    {"id": "keys", "label": "Nintendo Switch prod.keys", "role": "key", "shared": "switch.prod-keys"}
  ],
  "start": {"program": ["example.exe"], "args": ["--game={input:base}", "--keys={input:keys}"]}
}
```

This is an illustrative integration: use arguments that the actual project supports.

- Selecting a folder does not activate the keys input.
- Selecting an NSP/XCI activates `keys` because **this profile explicitly declared the dependency**.
- An NSP/XCI declaration without `requires` never creates a keys prompt.
- An imported compatible key file is reused automatically by every project declaring that type.
  There is no repeated opt-in per game. Missing files prompt once; replacement is available in
  Easy Setup. The UI explains that the central copy is local and used by compatible projects.
- A shared key not referenced by any format's `requires` is unconditional. Shared inputs cannot
  be optional features. Defer optional system resources rather than adding them here.
- Requirements refer only to shared-key input IDs. Recursive dependencies and arbitrary resource
  dependencies are not supported. File extension rules must not overlap.

The NSP container's name/extension does not establish encryption state. `prod.keys` is a keyset,
not a universal guarantee of successful decryption. Some packages need title rights from a ticket
or `title.keys`. The project decides what its reader needs and validates generation, rights ID,
game version and supported package layout. The launcher checks keys-file shape, not decryption.
A prepare failure remains a failure, preserves selected inputs, and allows changing the key file
before retrying. No guessed firmware-version database or automatic title-key extraction is added.

## Four discs and three optional DLC files

See [the complete example](examples/multidisc-inputs.json). Declare one input for each disc with
`role: "disc"`, and one for each DLC with `role: "dlc", "optional": true`. Required is the default.
`label` should say exactly which disc/version/content to choose. A `file` format may list accepted
extensions without a leading dot; an empty list accepts any regular file. A `folder` format accepts
a selected existing folder and cannot declare extensions or an installation destination.

You can pass every selected path to `prepare.args`, `start.args`, or `start.env` using `{input:id}`.
Paths remain separate command arguments (no shell string concatenation). For optional or
conditional arguments, **put the flag and token in the same argument**, e.g.
`"--dlc={input:dlc-one}"`. An entire argument/environment value containing an unavailable input
token is omitted. Do not use separate `"--dlc", "{input:dlc-one}"` arguments for optional inputs;
the dangling flag could consume another argument. Required inputs can use separate flag/value
arguments. Unknown placeholder IDs are configuration errors.

## File placement

Use `destination`, e.g. `"keys/prod.keys"` or `"content/disc1.iso"`, only when the project requires
a file in its installation. Tokens still refer to the selected source file. Paths cannot be
absolute, contain `..`, use placeholders, or traverse a symlink/junction outside the install.
Destinations must be unique. Existing unmanaged files are not overwritten. Managed copies are
reapplied before launch and after a setup/update. Removing optional content removes its managed
delivery copy on the next setup. Folders are passed by path; recursive folder staging is not part
of this extension.

Game files can be hard-linked (copied across volumes). Shared keys are always separate private
copies, never hard-linked to the user's original or the central master. Prefer a path argument
when a project supports it. No keys are staged into unrelated emulator directories, arbitrary
user folders or beside game packages outside the install.

## NVIDIA DLSS (`nvidia.dlss`)

NVIDIA's `nvngx_dlss.dll` isn't open source, so some recomps support DLSS but can't ship it. A
recomp asks for it once; the player provides nothing:

```json
{
  "inputs": [
    { "id": "dlss", "label": "NVIDIA DLSS", "role": "runtime", "shared": "nvidia.dlss",
      "optional": true, "destination": "nvngx_dlss.dll" }
  ]
}
```

- Role `runtime` (a file the launcher fetches itself) only, no `formats`, once per profile. Unlike
  the keys it may be optional. Leave `destination` out and pass `"--dlss={input:dlss}"` in
  `start.args` to get its path instead.
- Only on Windows or Linux (Proton) with an NVIDIA RTX card. Everywhere else the row isn't shown
  and `{input:dlss}` arguments are left out, as for any optional input that isn't there.
- recomplauncher.com/launcher/dlss.json names the version; the file is downloaded only from
  `raw.githubusercontent.com/NVIDIA/` or `github.com/NVIDIA/`, checked for its size and signature
  (NVIDIA Corporation), and kept once in `shared-runtime/nvidia.dlss/<version>/`. Each game gets
  its own managed copy. A file the game's release ships itself is never replaced.
- Easy Setup shows a compact optional row ("Downloaded from NVIDIA") the player can turn off per
  game. A failed download never stops setup: the game plays without DLSS.
- First in RecompLauncher 0.90 Beta 37. From that version, an **optional** input with a shared
  type or role the launcher doesn't know is skipped (its `{input:...}` arguments left out) instead
  of failing, so future shared types don't break launchers. A required one still fails.

## Persistence, replacement and privacy

Named game inputs live in the installed entry's `setup_inputs`. A preparation fingerprint covers
the declarations, prepare command and selected paths/metadata, **not disc contents**. Changing an
input, a shared key's metadata, a declaration or an installed version requires running setup
again. Keep source files available after setup if a project reads them at runtime.

Imported keys live under the launcher's private `shared-keys/<shared-type>/` directory. These are
not account credentials and do not use the RecompLauncher account token slot. Replacing keys is
blocked while any game/setup is running. Remove Shared Keys explains its cross-project effect
and removes the managed master plus identifiable managed delivery copies. User originals stay.

Managed inputs are excluded from save/cloud snapshots and update backups; key contents never
appear in launcher diagnostics. Inputs delivered by path stay outside game saves. Game-side
code runs as the user and can read any path it receives: this feature is file management, not an
application sandbox or trust certification.

## Validation

Run `cargo test --manifest-path src-tauri/Cargo.toml --lib inputs::tests` for the named-input backend
and `node --test tests/setup-inputs.test.cjs` for UI requirements, disclosure, and escaping checks.
For visual review, run `node tests/preview-server.cjs` and open
`http://127.0.0.1:4783/tests/setup-inputs-preview.html` (add `?tv` for Big Picture). The preview uses
simulated selections and never imports files or runs preparation. Its game/DLC labels are fixtures,
not a supported integration manifest.

- Maximum 32 inputs, unique IDs, nonempty labels and one declaration per shared type.
- Shared type/role and accepted formats validated by backend and documented in the JSON schema.
- Existing single-copy flows retained. New multidisc flow makes no verified-dump claim.
- Test required slots, optional slots, conditional keys, reuse/replacement/removal, retry/cancel,
  paths with spaces, safe destination placement, update persistence and backup exclusions.
- Keep desktop and Big Picture requirements identical. Adding BIOS/firmware support or content
  hashing as a convenience would violate the scope above.
