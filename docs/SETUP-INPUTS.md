# Named Easy Setup inputs and shared keys

This extension to format 1 lets a project ask for multiple discs, required updates and optional
DLC as separate named inputs. Easy Setup shows them in steps: **Game** (**Discs** when there are
several) with the required files and shared keys, **Add-ons** with the optional ones, and
**Extras** with `prepare.options` and NVIDIA DLSS. A step with nothing in it isn't shown, so four
discs and three DLC files are two pages, and a game that needs one file is one. Each file is a row
(or, when the game needs just one, a box) with its buttons on it; a file found or kept from before
shows there with a tag saying so. The disc artwork and two-column layout are kept; on smaller
windows the artwork sits above. Preparation starts only from Set Up, once every required input is
selected. Desktop and Big Picture draw the same steps (`ui/setup-steps.js`).

Named inputs are the one way to ask for files. **The top-level `game` field is deprecated** (still
read until RecompLauncher 1.0, as the input `game`); **the input named `game` is the new way.** The
player's copy is shown as the big box, and everything else is a row under it. Pass each one with
`{input:id}`. Names refer to input IDs, not shared-resource names.

Which input for which file:

1. **A game that is one file (or one folder):** one input, `"id": "game"`, `"role": "game"` (the
   default), passed as `{input:game}`.
2. **A game on several discs:** every disc has `"role": "disc"`, ids `disc1` to `discN`, labels
   `Disc 1` to `Disc N`, and no input has role `game`. The launcher makes Disc 1 the copy box by
   itself. Give every disc a `check`. Not every disc as role `game`: only the first would be the
   copy box, and the Discs step, the "Insert your discs" heading and the "N of N discs selected"
   count all go by role `disc`.
3. **DLC:** `"role": "dlc"`, `"optional": true`, one input per package (`dlc1`, `dlc2`, ...).
4. **An update:** `"role": "update"`, usually with `into` and `name`.
5. **Keys:** a `shared` input (`switch.prod-keys` ...), never a file of the game's own the player picks.
6. **A folder input only when the copy is a folder** (an extracted PS3 game, a Switch game's
   `exefs` and `romfs`); never a folder of discs or DLC. One file per input, one argument per
   file, the flag and the file together (`--disc2={input:disc2}`).

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
The project's prepare program must perform its own content validation. An input with `dumps` is
read whole and compared by CRC32 (see below); nothing else is.

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
The prod.keys shape check accepts bounded even-length hexadecimal records, including RSA/keyblob entries; the engine validates the specific key lengths and values it uses. Title-key values remain 16 bytes.
A prepare failure remains a failure, preserves selected inputs, and allows changing the key file
before retrying. No guessed firmware-version database or automatic title-key extraction is added.

## Four discs and three optional DLC files

See [the complete example](examples/multidisc-inputs.json). Declare one input for each disc with
`role: "disc"`, and one for each DLC with `role: "dlc", "optional": true`. Required is the default.
`label` should say exactly which disc/version/content to choose. A `file` format may list accepted
extensions without a leading dot; an empty list accepts any regular file. A `folder` format accepts
a selected existing folder and cannot declare extensions or an installation destination.

**One file per input, one argument per file.** Each disc image and each DLC package is an input of
its own and reaches the game through an argument of its own: `--disc1={input:disc1}`,
`--disc2={input:disc2}`, `--dlc1={input:dlc1}`. Never ask for a folder of discs or DLC and pass the
folder. A `folder` format is only for a copy that is a folder: an extracted PS3 game, or a Switch
game's `exefs` and `romfs`.

You can pass every selected path to `prepare.args`, `start.args`, `start.env`, or a game's
`games[].args` using `{input:id}`.
Paths remain separate command arguments (no shell string concatenation). For optional or
conditional arguments, **put the flag and token in the same argument**, e.g.
`"--dlc={input:dlc-one}"`. An entire argument/environment value containing an unavailable input
token is omitted. Do not use separate `"--dlc", "{input:dlc-one}"` arguments for optional inputs;
the dangling flag could consume another argument. Required inputs can use separate flag/value
arguments. Unknown placeholder IDs are configuration errors. When one game of several is played,
the other games' inputs aren't unknown: their arguments are left out, so only that game's
`"--rom={input:rom-blue}"` reaches the command line.

## Checking, placing and unpacking a file

An input can say how to tell it's the right game, and where the launcher puts it. These are the
options `game` had, so anything `game` did, an input does too:

```json
{
  "format": 1,
  "inputs": [
    {
      "id": "game",
      "label": "Your PS2 disc image (.iso) of the USA release",
      "formats": [{"type": "file"}, {"type": "folder"}],
      "check": {"iso9660": "SLUS_206.51"},
      "dumps": [{"name": "USA", "crc32": "0011aabb", "size": 4700372992}],
      "into": "game",
      "contains": ["SYSTEM.CNF"],
      "extract": "iso9660"
    },
    {
      "id": "update", "label": "Your title update (TU 1.03)", "role": "update",
      "formats": [{"type": "file"}], "into": "game/Content", "name": "tu00000003.bin"
    }
  ],
  "start": {"program": ["game.exe"], "args": ["--game_data_root={input:game}"]}
}
```

- `check` (`file`, `iso9660`, `xdvdfs`, `nup_build_id`, `ids`): checked when the player picks it;
  another game's copy is refused with a plain reason. Never again on Play.
- `dumps`: known good dumps (CRC32). Only an input with these is read whole on setup, for the
  verified badge; any other is checked by its shape and `check` only.
- `into` and `name`: placed in that folder of your release before each start (links, or a copy on
  another drive). `contains` lists what the folder must hold; files the player already put there
  count as picked. Use `destination` instead for a file the launcher copies in and keeps.
- `extract`: a disc image unpacked on setup into `into` (or `game-files`): `xdvdfs` (Xbox, Xbox
  360) or `iso9660` (PS2, PSP).
- With role `update`, `sha256` (the one known file) or `name` checks the file when it's picked.

A field the launcher doesn't know is ignored, so an input written for a newer launcher still reads.

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
- Easy Setup shows it on the Extras step ("Downloaded from NVIDIA"), with a switch the player can
  turn off per game. A failed download never stops setup: the game plays without DLSS.
- First in RecompLauncher 0.90 Beta 37. From that version, an **optional** input with a shared
  type or role the launcher doesn't know is skipped (its `{input:...}` arguments left out) instead
  of failing, so future shared types don't break launchers. A required one still fails.

## Persistence, replacement and privacy

The files the player picked live in the installed entry's `setup_inputs`, by input id. What
Easy Setup keeps once a setup has worked is those picks and the release it finished on. Picks made
on the setup screen wait there until a setup with them works: closing it without one leaves the
game as it was. Files are checked when they're picked, never again on Play.

A game with a setup step (or a disc the launcher unpacks) is set up while the release it finished
on is the one installed. A new release (or going back to an older one) runs the setup again before
it plays; reinstalling the same one doesn't. Picking another file runs it again too. A game without
a setup step is set up once its required files are picked: Play puts them where the game reads them
each time. When the step's output lives in the game's folder, list it in `prepare.done`; if an
update takes it away, Play asks for setup again instead of starting without it. A built game
needs only the files its `start` reads (or that are placed in its folder); the rest may be moved
or deleted. Keep source files available after setup if a project reads them at runtime.

New shared keys are used by each game at its next setup: picking them never sets another game up
again.

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
and `node --test tests/setup-inputs.test.cjs tests/setup-steps.test.cjs` for UI requirements,
disclosure, and escaping checks. For visual review, run `node tests/preview-server.cjs` and open
`http://127.0.0.1:4783/tests/setup-steps-preview.html` (add `?tv` for Big Picture): every step of
several cases, from one file to four discs with DLC, keys and a required option. The preview uses
simulated selections and never imports files or runs preparation. Its game/DLC labels are fixtures,
not a supported integration manifest.

- Maximum 32 inputs, unique IDs, nonempty labels and one declaration per shared type.
- Shared type/role and accepted formats validated by backend and documented in the JSON schema.
- A multidisc set makes no verified-dump claim; each disc with `dumps` is checked on its own.
- Test required slots, optional slots, conditional keys, reuse/replacement/removal, retry/cancel,
  paths with spaces, safe destination placement, update persistence and backup exclusions.
- Keep desktop and Big Picture requirements identical. Adding BIOS/firmware support or content
  hashing as a convenience would violate the scope above.
