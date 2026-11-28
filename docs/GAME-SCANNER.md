# How the launcher finds the player's games

The player adds their ROM folders in Settings (Easy Setup, "Where do you keep your ROMs?", or
"From My Emulators" to take them from Dolphin, PCSX2, RPCS3, RetroArch, Cemu and Ryujinx), and
the launcher looks through them for the games they already own: ROMs, disc images and extracted
games. A game it finds there shows as owned in
the catalog, and Easy Setup fills in its copy. Nothing is uploaded: everything is read on the PC.

This page lists what the scanner reads today, for each console, so gaps are easy to see and fill.
The code is [`src-tauri/src/roms.rs`](../src-tauri/src/roms.rs). **When you change what it finds,
update this page in the same commit.**

## What it finds, for each console

- **Found:** the files and folders it picks up. Anything else in the folder is skipped.
- **Console from:** how it knows which console the file is for. Where that's only the folder
  name, keep the file in a folder named for its console (see [Folder names](#folder-names)).
- **ID read:** the ID the launcher reads from the copy itself, compared with the catalog's
  `copy_ids` and a recomp's `check.ids` (see [DEVELOPERS.md](DEVELOPERS.md#inputs-the-players-copy)).
  Without one, the copy is matched by its name only.
- **Not found yet:** common formats the scanner skips today. They're the places to improve.

| Console (`origin`) | Found | Console from | ID read | Not found yet |
| --- | --- | --- | --- | --- |
| Nintendo 64 (`n64`) | `.z64`, `.n64`, `.v64` (any byte order); one inside a `.zip` or `.7z` | File type | Game code from the header (`NZSE`). The header's title and region are read too. Nothing is read from a zipped ROM | `.ndd` (64DD) |
| GameCube (`gc`) | `.iso`, `.gcm`, `.rvz`, `.wia`, `.wbfs`, `.ciso` | The disc header | Disc ID (`GZLE01`), also inside WBFS, CISO, WIA and RVZ | `.gcz`, `.tgc`, `.dol`/`.elf`, extracted folders |
| Wii (`wii`) | Same as GameCube | The disc header | Disc ID (`RMGE01`) | `.wad`, extracted folders |
| Wii U (`wiiu`) | `.wua`, `.wux`, `.wud`, each checked by its own header | File type | Title ID from a `.wua` (`00050000...`). Not read from `.wux` or `.wud` | Extracted folders (`code`/`content`/`meta`), `.rpx` |
| Switch (`switch`) | `.xci`, `.nsp`; an extracted game (`exefs/main.npdm` and `exefs/main`, or the same under `base/`) inside a folder named `Switch` | File type; the `Switch` folder for an extracted game | Title ID from an `.nsp`'s ticket, else from the file name (`[01007300020FA800]`); from `main.npdm` for an extracted game; from the name only for an `.xci` | `.nsz`, `.xcz` |
| Switch 2 (`switch2`) | `.nup`; `.xci` and `.nsp` too; an extracted game inside a folder named `Switch2` or `Switch 2` | File type (a `.nup` fits Switch or Switch 2) | Title ID from the file name (`04...`). The `nup_build_id` check is separate (see DEVELOPERS.md) | |
| 3DS (`3ds`) | `.3ds`, `.cci`, `.cia`, `.cxi`; one in a zip or 7z | File type | None | `.3dsx`, `.app` |
| DS (`ds`) | `.nds`; one in a zip or 7z | File type | None | `.dsi` |
| Game Boy, Color, Advance (`gb`, `gbc`, `gba`) | `.gb`, `.gbc`, `.gba`; one in a zip or 7z. A GB file fits a GBC recomp and the other way round | File type | None | `.sgb` |
| NES (`nes`) | `.nes`, `.fds`, `.unf`; one in a zip or 7z | File type | None | |
| SNES (`snes`) | `.sfc`, `.smc`; one in a zip or 7z | File type | None | `.bs`, `.st` |
| Virtual Boy (`vb`) | `.vb`, `.vboy`; one in a zip or 7z | File type | None | |
| Genesis / Mega Drive (`genesis`) | `.md`, `.gen`, `.smd`; one in a zip or 7z | File type | None | `.bin` (too common a type to scan) |
| 32X (`s32x`) | `.32x`; one in a zip or 7z | File type | None | |
| Game Gear, Master System (`gg`, `sms`) | `.gg`, `.sms`; one in a zip or 7z | File type | None | |
| Amiga (`amiga`) | `.adf`, `.hdf`, `.ipf`, `.lha`; one in a zip or 7z | File type | None | `.dms`, WHDLoad folders |
| PlayStation (`ps1`) | `.iso`; a `.cue` and its `.bin` tracks; an `.m3u` of discs; `.chd` | What's on the disc (ISO 9660 saying PLAYSTATION, `BOOT` in SYSTEM.CNF); a `.chd` by its folder name, else it fits any disc console | Serial from SYSTEM.CNF (`SLUS00594`), also through a `.cue` or `.m3u` | A `.bin` with no `.cue`, `.pbp`, `.ecm`, `.img` |
| PlayStation 2 (`ps2`) | `.iso`; a `.cue` and its `.bin` tracks; `.chd` | What's on the disc (`BOOT2` in SYSTEM.CNF); a `.chd` by its folder name, else it fits any disc console | Serial from SYSTEM.CNF (`SLUS20312`) | `.cso`, `.zso`, `.gz` |
| PSP (`psp`) | `.iso` | What's on the disc (`PSP GAME`) | None | `.cso`, `.pbp` (PSN games) |
| PlayStation 3 (`ps3`) | An extracted disc game (`PS3_DISC.SFB`, `PS3_GAME/USRDIR/EBOOT.BIN`); an installed or extracted PSN game (`PARAM.SFO`, `USRDIR/EBOOT.BIN`, `NP...` ID); a `.pkg`; an `.iso` | The folder's files; the `.pkg` header; for an `.iso`, a PS3 folder name, else it fits any disc console | Title ID from `PARAM.SFO` (`BLUS30443`) or the `.pkg`'s content ID. Not read from an `.iso` | Reading a PS3 `.iso` itself |
| PlayStation 4 (`ps4`) | An extracted game (`eboot.bin` and `sce_sys/param.sfo`, `CUSA...` ID) | The folder's files | Title ID from `param.sfo` (`CUSA00242`) | `.pkg` (PS4 packages are skipped) |
| PlayStation 5 (`ps5`) | An extracted game (`eboot.bin` and `sce_sys/param.json`, `PPSA...` ID) | The folder's files | Title ID from `param.json` (`PPSA21559`) | |
| Xbox (`xbox`) | `.iso` | What's on the disc (XDVDFS with `default.xbe`) | None | `.xiso` file type, extracted folders (`default.xbe`) |
| Xbox 360 (`x360`) | `.iso`; an extracted game (the folder holding `default.xex`); Xbox Live Arcade games, title updates and DLC (CON, LIVE and PIRS packages, usually with no file type) | What's on the disc (XDVDFS with `default.xex`); a package's header | Title ID from `default.xex` (`454107EC`), in a disc image or a folder; from a package's header | Games on Demand packages (their ID is read when a recomp checks a picked one, but the scan skips them), `.xex` files not named `default` |
| Saturn (`saturn`) | `.iso`; a `.cue` and its `.bin` tracks; `.chd` | What's on the disc (`SEGA SEGASATURN`); a `.chd` fits any disc console | None | A Saturn folder name, `.mdf` |
| Dreamcast (`dc`) | `.gdi` and its tracks; `.cue`; `.iso`; `.chd` | What's on the disc (`SEGA SEGAKATANA`); a `.gdi` is a Dreamcast disc; a `.chd` by its folder name, else it fits any disc console | None | `.cdi` |
| Arcade (`arcade`) | A MAME-style set: a `.zip` or `.7z` with a short name (`hopr`) in a folder named arcade, MAME, FBNeo, FBA or FinalBurn | The folder name | The set's name, which has to be listed whole in `copy_ids` | `.chd` disks that go with a set |

A recomp whose `origin` is empty, `other` or `arcade` takes any file that matches by name.

## Rules that apply to every console

- **Size:** a disc image has to be over 50 MB and a cartridge ROM at least 8 KB, so small files
  that happen to share a file type aren't taken. N64 ROMs and Xbox 360 `default.xex` have no
  minimum; sheets, packages, Wii U files and archives are checked by their headers instead.
- **Disc sets:** a `.cue`, `.gdi` or `.m3u` stands for its tracks or discs. It's found once, and
  the files it lists aren't listed again on their own. Its size is its tracks'. A sheet over 64 KB
  isn't read.
- **Zips and 7z:** only the archive's file list is read, nothing is unpacked. It counts only when
  it holds exactly one cartridge or N64 ROM, or it's an arcade set. Disc images inside an archive
  aren't found.
- **Extracted games** (PS3, PS4, PS5, Switch) are found as one folder, and what's inside them
  isn't looked through.
- **Skipped folders:** names starting with `.`, `node_modules` and `windows`. A linked folder is
  looked through once. A scan stops after a million files.
- **Region** is only read from N64 headers. When several copies match, a US one is picked first.

## Folder names

Some files don't say which console they're for (a `.chd`, or an `.iso` the scanner can't read
inside). Then the nearest folder named for a console decides. With none, the file fits any
console that uses discs and is matched by name only. Folder names are matched as whole words, any
case, with `_` and `-` read as spaces:

| Console | Folder names |
| --- | --- |
| Xbox 360 | `Xbox 360`, `Xbox360`, `X360` |
| Xbox | `Original Xbox`, `OGXbox`, `Xbox` |
| PlayStation 2 | `PlayStation 2`, `PlayStation2`, `PS2` |
| PlayStation 3 | `PlayStation 3`, `PS3` |
| PSP | `PlayStation Portable`, `PSP` |
| PlayStation | `PlayStation 1`, `PSX`, `PS1`, `PSOne`, `PlayStation` (fits PS1 or PS2) |
| GameCube | `GameCube`, `NGC`, `GC` |
| Wii U | `Wii U`, `WiiU` |
| Wii | `Wii` |
| Dreamcast | `Dreamcast` |
| Arcade | `Arcade`, `MAME`, `FBNeo`, `FBA`, `FinalBurn` |

So `Sony - PlayStation 2` and `ps2_games` both count. Switch and Switch 2 extracted games use their
own check: a folder named exactly `Switch`, `Switch2` or `Switch 2` (any case) above the game. A
game with a plain `exefs` (no `base/`) has to sit right inside that folder, so an update kept
next to its game isn't taken for one.

## How a found file is matched to a recomp

1. **Console.** The file has to be for the recomp's `origin` (from the table above). A file the
   scanner couldn't place fits any console that uses discs, never a cartridge one.
2. **Name.** Every word of the catalog's game name has to be in the file's title or file name,
   and at least half the file's words have to be the game's. Bracketed tags (`(USA)`, `[v0]`),
   apostrophes and small words (the, of, a, and, usa, us, europe, rev, v1) are ignored. A sequel
   number the game's name doesn't have rules it out (`Gears of War 2` isn't `Gears of War`), but
   `Disc 2` or `Part 2` doesn't.
3. **ID, by itself.** An extracted PS3, PS4 or PS5 game and an arcade set also match when the
   catalog's `copy_ids` lists their ID, since their own titles often differ from the catalog's.
4. **ID check.** When the catalog lists `copy_ids` or the recomp lists `check.ids`, a copy whose
   ID was read and is another game's is refused. A shorter listed ID matches every ID that starts
   with it (`GZL` fits every region of `GZLE01`). A copy whose ID can't be read isn't refused
   for it.
5. **The recomp's own check.** For recomps the launcher sets up, the copy also has to pass the
   recomp's `check` (see [DEVELOPERS.md](DEVELOPERS.md#inputs-the-players-copy)).

An Xbox Live Arcade package is the game itself. Xbox 360 updates and DLC are never picked as the
game: they're kept as that game's extra content.
Whether a file is the game, an update or DLC, and which disc it is, is read from its name
(`(Disc 2 of 3)`, `[UPD]`, `Title Update`, `DLC`, a Switch title ID ending in `800` for an update),
or from the header for Xbox 360 packages and PS3 update `.pkg` files.

## Adding a format

1. Add the file type or folder layout in `roms.rs` (`identify_by_type`, or `playstation_game` /
   `identify_switch` for extracted games) and, if it can carry an ID, in `copy_id`.
2. Read as little as you can: headers and file lists, never the whole file, and nothing that
   needs keys.
3. Add a test next to the others in `roms.rs`.
4. Move the format from **Not found yet** to **Found** in the table above, and fill in
   **Console from** and **ID read**.
