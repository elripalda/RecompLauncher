# Folder Structure

Settings → Games and folders → **Folder Structure** sets the one folder layout installed games go
in on disk, for example `N64/Ocarina of Time/Ship of Harkinian/` inside the library. It changes
only where game folders sit. The Library, its sorting and filters, game pages and how games start
stay as they are.

The flow is: choose a preset, change it if you like (drag fields into folder levels), check the
preview, save, and optionally **Reorganize Existing Folders**.

## What the launcher actually knows about a game

Each field was checked against what the launcher has (the catalog, `pc-catalog.json`, and the
Library, `library.json`) before it was offered.

| Field | Where it comes from | Have it? |
|---|---|---|
| Console | catalog `origin` and its name in `origins` | Catalog games: yes. Games added from GitHub: no |
| Manufacturer | worked out from the console (built-in table in `folders.rs`) | Whenever the console is known (not for Arcade/Other) |
| Game title | catalog `game`, else its `titles[]` group name, else the Library's `game` | Almost always |
| Recompilation name | the Library's name | Always |
| Recompilation year | year of the catalog's `first` (first release date) | Catalog games: yes |
| Recompilation team | developer page name (`developers[]`), else `author`, else the repo owner | Almost always |
| Genre | catalog `genre` | Most catalog games |
| Technology/type | a catalog tag such as decomp, port, recomp | Only when tagged |
| Region | the N64 cartridge header of a copy the player linked | N64 only |
| Installation source | the repo address (GitHub, GitLab, Forgejo, SourceForge) or Local Build | Always |
| Alphabetical | first letter of the game title ("The"/"A" skipped; digits go to `0-9`, anything else to `#`) | Always |
| Version | the installed release | Always (it changes with updates) |
| Franchise, original release year, original developer, publisher | **not in the catalog today** | Only from the player's per-game values. Read from the catalog too (`franchise`/`series`, `year`, `developer`, `publisher`) if it ever carries them |
| Custom text | typed by the player | Always |

## When a field is missing

Every field has a fallback, applied the same way every time:

- **Use words**: a fixed folder name instead (default `Unknown Year`, `Unknown Developer`,
  `Unknown Publisher`, `Unknown Team`, or `Other` for console, manufacturer, genre and technology).
- **Leave it out**: the field is dropped from the folder name; a folder level with nothing left in
  it is dropped too. Default for franchise and region, so most games aren't put in an "Unknown"
  folder for an extra hardly any game has.
- **Use another field**: that field's value instead (one step only, so two fields can't point at
  each other forever). Default for game title, which falls back to the recompilation name.

Order of precedence for a value: the player's own per-game value, then what the catalog/Library
has, then the fallback. With every level left out, the game gets its usual folder (its id).

## Names

- Folder names are made safe for every system: `:` becomes ` -`, `/ \ |` become `-`, `< > " ? *`
  are dropped, Windows' reserved names (CON, NUL, COM1…) get a `_`, no leading/trailing dots or
  spaces, at most 64 characters.
- Console names: short (N64, PS2) or full (Nintendo 64, PlayStation 2), and the player's own name
  per console. Common spellings are recognised as the same console (Super Famicom → SNES,
  Mega Drive → Genesis, PlayStation 2 → PS2).
- Capitalization: as written, Title Case, UPPER or lower.
- Two games that would share a folder (two recompilations of one game under Year → Console →
  Game): the second gets ` - <recompilation name>` added, then ` (2)`, ` (3)`… Games are taken in
  id order so the same Library always gives the same names.

The structure and the per-game values are kept in `folders.json` beside `library.json`, so a
catalog refresh or a rescan never resets them.

## New installs

**Automatically Use My Folder Structure for New Installations** (off by default): a newly
installed game goes in its folder of the structure. Updates always stay in the game's current
folder. Off: the launcher's usual folder (`<library>/<id>`). Changing the structure never moves
anything by itself.

## Reorganize Existing Folders

1. Lists every installed game: its folder now, where the structure puts it, and whether it moves.
2. A game stays where it is (and says why) when it's running, installing/updating, a local build,
   in a shared folder (a drive, Downloads…), missing, or when its new folder already has files.
3. A game in a folder outside the libraries is listed unticked: it moves only if ticked.
4. Nothing moves before the player confirms.
5. Each move is a rename on the same drive; across drives the copy is finished before the old
   folder is removed. The game's saves and settings inside its folder go with it, its `.backups`
   follow, and paths the Library keeps inside the folder (its copy, setup inputs…) are updated.
   After the move the game's program must be there, or it's put back.
6. Each move is written to `folders-journal.json` before it starts. At the next start, a move the
   launcher was closed in the middle of is finished (the new folder is whole) or undone (only the
   half copy is removed, never the game's own folder).
7. **Undo Last Reorganize** moves the games back, each only if its old folder is still free.

Empty structure folders left behind by a move or an uninstall are removed (empty ones only, never
the library itself). Uninstall deletes a game's folder in the structure like one made the usual way.

## Known limits

- Franchise, original year, original developer and publisher need per-game values until the
  catalog carries them.
- A desktop shortcut made with **Shortcuts Go Through the Launcher** off points at the program
  itself; after a move, make it again (as with Move to another drive).
- Version in a folder name changes with each update; reorganizing again renames the folder.
