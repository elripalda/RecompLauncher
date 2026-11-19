# Works with RecompLauncher

For multiple discs, DLC slots and explicitly conditional shared keys, see
[Named Easy Setup inputs](SETUP-INPUTS.md). That extension is limited to game inputs and three
shared key types. BIOS/firmware management and multidisc hashing are explicitly deferred.

RecompLauncher can set your recomp up for the player. They drop in their disc image, watch it get
checked and set up, change your video settings on the game's page, and press Play. They never
open a config file or your own launcher.

All of it comes from one file you write, `recomplauncher.json`. Your recomp doesn't have to
change: the launcher only does what a player would do by hand. It edits your settings file,
passes command-line arguments and starts your executable.

It has four parts. Add the ones your game needs: a game that reads the player's disc while it
runs is completely set up without a `prepare` step.

| Part | What players get | What you add |
| --- | --- | --- |
| Launch | Drop in a disc, it's checked and set up, Play starts the game directly | `game` and `start` |
| Settings | Your video and gameplay settings on the game's page and in Big Picture mode | `config` and `settings` |
| Preparation | Your own setup step that runs once before Play | `prepare` |
| Verification | A "verified" badge when the player's copy matches a known dump | `dumps` |

The rule: use `start` to pass inputs the game reads when it starts. Add `prepare` only when your
game has separate work to do before Play, like importing assets or recompiling from the player's
dump. Verification works with either.

Game pages show who maintains a game's setup: its developer, RecompLauncher or the community.
Where the file comes from doesn't decide it. If you ship and maintain your own
`recomplauncher.json`, tell us and your game shows as maintained by its developer.

Launch takes about ten minutes. Before any of them, your releases need to install cleanly, which
most already do: see [Releases that install cleanly](#releases-that-install-cleanly).

## Releases that install cleanly

The launcher installs and updates your recomp straight from your GitHub releases. Nothing goes
through us, and every release you publish reaches players the next time the launcher checks.

- **One file per system.** Put the system and the processor in the name:
  `MyRecomp-v1.2.0-Windows-x64.zip`, `MyRecomp-v1.2.0-Linux-x64.AppImage`. The launcher picks the
  file for the player's PC from those words. Builds for ARM say `arm64`.
- **Zips on Windows.** A `.zip` (or `.7z`) of your folder is best. An installer `.exe` works, but
  it's the last choice, because it wants to be run as setup. On Linux, an AppImage or a `.tar.gz`.
- **One folder on top is fine.** A zip with everything inside `MyRecomp-v1.2.0/` is unwrapped, so
  your program lands in the game's folder either way.
- **Keep extras out of the way.** Files with `debug`, `symbols`, `pdb` or `source` in the name, and
  `.sha256`, `.sig` and `.asc` files, are never taken for the game.
- **Checked on the way in.** GitHub keeps a SHA-256 of every file you upload, and the launcher
  checks the download against it before installing. You don't have to do anything.
- **Updates keep what the game made.** An update unpacks over the game's folder, so saves,
  settings and caches your game wrote stay. That also means a settings file *inside your release*
  replaces the player's on every update. Ship defaults instead (`default.toml`, or write them on
  first start), or let the launcher write them with `config.new`.
- **Updates go in whole or not at all.** A file your last release had and the new one doesn't
  goes with the update, unless the game changed it. If anything stops an update (a locked file, a
  full drive, the PC turning off), the game is left exactly as it was, and an update cut off by the
  launcher closing is finished or undone the next time it starts. Your release's files are only
  ever all old or all new. A release that has a file where the last one had a folder (or the other
  way round) replaces it only when it holds nothing but your last release's files; when the
  player's own files are there, the update stops and says so, and nothing changes.
- **`recomplauncher.json` at the top of the zip**, next to your program.
- **Pre-releases are betas.** Mark a test build as a pre-release on GitHub. Normal players stay on
  your latest full release, and we can open the pre-release as a beta in the launcher, where
  players join it with one button and go back just as easily. See [Betas](#betas).

## Where the file goes

The launcher uses the first one it finds:

1. **Inside your AppImage.** Put `recomplauncher.json` at the AppDir root before packaging a
   type-2 AppImage. It belongs to that exact build and takes precedence over an adjacent file.
2. **Your release folder.** Put `recomplauncher.json` at the top of your release zip, next to your
   executable. It also provides a fallback for AppImages without valid embedded metadata.
3. **The catalog.** If you can't ship it, send it to us and we'll add it to your entry on
   recomplauncher.com.
4. **Your repo, on a Forgejo or Gitea server.** For a project hosted there, the launcher also reads
   `recomplauncher.json` from the top of the repo's default branch
   (`https://{server}/api/v1/repos/{owner}/{name}/raw/recomplauncher.json`) when neither of the
   above has one. For example, `git.example.dev/studio/game` with the file next to its README.

### AppImages

The launcher reads `/recomplauncher.json` directly from the AppImage's SquashFS payload.
It does not execute or mount the image to inspect it, and writes no extracted JSON sidecar.
Settings, save and folder paths still start from the installation directory (`{install}`),
not from the internal filesystem.

The Flatpak build of the launcher unpacks your AppImage at install and starts its `AppRun`. See
[Your recomp in the RecompLauncher Flatpak](FLATPAK.md) for that and the sandbox's other
differences.

Use the AppImage's filename in `start.program`, for example:

```json
"start": {
  "program": ["MyGame-x86_64.AppImage"],
  "args": ["{game}"]
}
```

Program entries ending in `.AppImage` in `start` and `prepare` are rebound to the actual
installed filename, so renaming the download does not break launch. An internal path such
as `usr/bin/game` cannot be started by the launcher outside the image.

Embedded lookup applies when the install folder contains exactly one regular `.AppImage`
file at its top level (case-insensitive extension). With multiple AppImages it does not
guess: the adjacent profile and catalog fallback continue to work. The manifest must be
a regular file no larger than 1 MiB, with `format: 1`. Gzip, XZ, Zstandard and LZ4 SquashFS
compression are supported. Missing, malformed or unsupported metadata falls back to the
adjacent manifest and then the catalog. Replacing the image is reflected on the next read;
there is no extracted copy to become stale.

Developer tools identify the embedded source and check the profile actually in use.

The same file uploaded twice, once without the version for `releases/latest/download` links
(`Game-1.2.0-x86_64.AppImage` and `Game-x86_64.AppImage`), is one build: the launcher installs
the one that names its version and doesn't ask the player to choose. GitHub's SHA-256 of each file
says they're the same.

### Mac apps

Put your `.app` at the top of a zip, with `recomplauncher.json` beside it, and say the system in
the name (`MyGame-1.2.0-macos-arm64.zip`) so it's picked over a disk image. Links inside the app,
like a framework's `Versions/Current`, are kept. Name the app in `start.program` and
`prepare.program`; the launcher runs the program its `Info.plist` names:

```json
"start": { "program": ["MyGame.app", "MyGame-x86_64.AppImage"] },
"prepare": { "program": ["MyGame.app", "MyGame-x86_64.AppImage"], "args": ["--prepare", "--game", "{game}"] }
```

The first entry that exists on the player's system runs, so one file covers a Mac and Linux.
RecompLauncher 0.90 Beta 54 and older find an app in `start.program` and `prepare.program` only by
its program's own path, which works everywhere: `"MyGame.app/Contents/MacOS/MyGame"`.

If your app updates itself (Sparkle, say), leave that off when `RECOMPLAUNCHER` is set: the
launcher installs each of your releases over the app, and two updaters on one app fight.

Two examples are worth reading:

- [fifawc14.json](examples/fifawc14.json): 2014 FIFA World Cup (Xbox 360), with a settings file
  and eight settings
- [streetball.json](examples/streetball.json): NBA Street Vol. 2 (PS2), with settings passed on
  the command line

Add `"$schema": "https://recomplauncher.com/schema/recomplauncher.json"` at the top and your
editor will complete fields and flag mistakes as you type. The schema is also in this folder:
[recomplauncher.schema.json](recomplauncher.schema.json).

## Launch: plays from RecompLauncher

```json
{
  "$schema": "https://recomplauncher.com/schema/recomplauncher.json",
  "format": 1,
  "repo": "you/your-recomp",
  "game": {
    "kind": "iso",
    "label": "Your disc image (.iso) of the USA release, serial SLUS-20651.",
    "check": { "iso9660": "SLUS_206.51" }
  },
  "start": {
    "program": ["bin/game.exe", "game.exe"],
    "args": ["--iso", "{game}"]
  }
}
```

What the player sees:

1. They install your recomp. If the launcher already found their copy in one of their game
   folders, it offers that one. Otherwise they drop the disc image on the setup screen, or pick
   it or its folder.
2. The setup screen reads the disc, checks it's your game, reads the whole image to make sure
   it's complete, and writes your settings.
3. "Ready to play", with Play and Settings. Play starts `start.program` straight away.

### `game`: the player's copy

- `kind`: `"iso"`, `"folder"` or `"iso_or_folder"`.
- `label`: what to ask for. Name the region or serial if only one works.
- `check`: how to tell it's the right game. Use whichever fits:
  - `file`: a file that should be in the folder, like `"default.xex"`.
  - `xdvdfs`: a file on an Xbox 360 disc image.
  - `iso9660`: a file in the top folder of a PlayStation 2 (or any ISO 9660) disc image. Raw CD
    sectors (a PlayStation `.bin`) work too, and a `.cue`, `.gdi` or `.m3u` is checked through
    its data tracks.
  - `nup_build_id`: the build ID of a Nintendo Switch or Switch 2 `.nup` package, e.g. `"e0f3ccfa"` (hex,
    at least 8 digits; a prefix of the full 64-digit ID is fine). It is read from the `exefs/main`
    of the package's newest title, from the table rows leading to it and the first 96 bytes of that
    file only, never the whole package, and a folder with `exefs/main` is read the same way. The
    file is recognised by its contents, not its extension. This is a content check only: it needs
    and adds no keys, and nothing is copied. A wrong build is refused with the build it found. Use
    it with `kind: "iso_or_folder"`.

The same check is how the launcher finds copies in the player's folders, so a disc image named
`backup-07.iso` is still found.

**CD games kept as several files.** The launcher finds a `.cue` (with its `.bin` tracks), a `.gdi`
or an `.m3u` as one copy. If your game reads that sheet, name it in `label`, and `{game}` is the
sheet. Otherwise `{game}` is the disc's first data track, the `.bin`, so a game that reads single
disc images still gets one:

```json
"game": {
  "kind": "iso",
  "label": "Your dump of the USA release (SLUS-00797): pick the .cue file, with its .bin tracks beside it.",
  "check": { "iso9660": "SLUS_007.97" }
}
```

Leave `game` out if your recomp finds the game itself.

**A second pick: the game's update.** When the game needs something beside the player's copy,
picked on its own (a title update, or a PS4 update folder kept apart from the game), add `update`
to `game`. The launcher asks for it after the copy and puts it in your folder:

```json
"game": {
  "kind": "folder",
  "label": "Your game folder (CUSA00341).",
  "check": { "file": "eboot.bin" },
  "update": { "into": "update", "label": "Your 1.02 update folder.", "accepts": "folder", "optional": true }
}
```

- `into`: the folder inside your release it goes in.
- `label`: what to ask for.
- `accepts`: `"file"` (the default), `"folder"` or `"file or folder"`.
- `optional`: the game plays without it, so the player can skip it.

### `start`

- `program`: paths inside your release, tried in order. The first one that exists runs.
- `args`: always passed. Players can add their own launch options after yours.
- `env`: environment variables for your game, written as names and values. Paths fill in like
  everywhere else. Names are letters, digits and `_`; system ones like `PATH` can't be set.

```json
"start": {
  "program": ["MyRecomp.exe"],
  "args": ["--iso", "{game}"],
  "env": { "MYRECOMP_DATA": "{documents}/My Recomp" }
}
```

Every game the launcher starts also gets these, whether it has a `recomplauncher.json` or not:

| | |
| --- | --- |
| `RECOMPLAUNCHER` | `1` |
| `RECOMPLAUNCHER_VERSION` | The launcher's version, like `0.75.0-beta.1` |
| `RECOMPLAUNCHER_MODE` | `bigpicture` when Play was pressed in Big Picture mode, else `desktop` |
| `RECOMPLAUNCHER_PAD` | The controller plugged in: `xbox` or `playstation` |

Use them to fit in: skip your own launcher window, open full screen with your controller menus
when the mode is `bigpicture`, and show the right button glyphs from the first frame.

### Paths

These fill in anywhere a path, an argument or a value is written:

| | |
| --- | --- |
| `{install}` | Your game's folder |
| `{game}` | The player's copy (the .iso or the folder; for a .cue, its .bin unless `label` names .cue) |
| `{documents}` | Documents |
| `{local}` | AppData\Local, or ~/.local/share |
| `{config}` | AppData\Roaming, or ~/.config |
| `{home}` | The user's home folder |
| `{pad}` | The controller plugged in: `xbox` or `playstation` |

Where a file can be in more than one place, write a list. The first entry whose `if` exists is
used, and an entry without `if` is the fallback:

```json
"file": [
  { "path": "{install}/game.toml", "if": "{install}/portable.txt" },
  { "path": "{documents}/My Game/game.toml" }
]
```

### `saves`

Where your game keeps its saves. The launcher copies them after every session (the newest ten stay
on the player's PC), and players who sign in can send them to their account and pick up on another
PC. List folders, single files, or files matching a `*` pattern:

```json
"saves": ["{local}/My Game/saves", "{install}/settings.toml"]
```

Without it (and without a catalog entry that says), the launcher doesn't guess: the game's Saves tab
says it doesn't know where the game saves, and nothing is backed up or synced until the player picks
the folder. So say where. Only list saves and small settings: game files, programs and archives are
never copied, and nothing over 64 MB.

You can move them in a later release. Keep the same number of entries in the same order, and change
the path in place: a copy made from the old place goes into the new one when the player restores it,
or when the launcher brings back their cloud save because the new place is empty. From
`"saves": ["{home}/Games/MyGame/user"]` to this, a copy from before restores into the new folder:

```json
"saves": ["{install}/Game/user"]
```

This only follows places your file or the catalog names, not ones the launcher noticed on its own.

### `games`: several games in one release

A release that carries a whole series lists its games. Players see each one as a game of its own:
in Discover, search and Home, with its own page (a "Part of" pill leads to the release's page) and
its own tile in the Library and Big Picture, with its own play time, achievements, pins and
shortcuts. Installing, updating and removing stay one thing for the whole release.

```json
"games": [
  { "id": "gow2", "name": "Gears of War 2", "args": ["--game=gow2"], "title_id": "4D53082D", "inputs": ["disc2"] },
  { "id": "gow3", "name": "Gears of War 3", "args": ["--game=gow3"], "title_id": "4D530906", "inputs": ["disc3"] }
]
```

- `id`: 1 to 16 of `a-z` and `0-9`, unique in your release. The game's full id is
  `<recomp id>--<id>`, used for its achievements, play time and links
  (`recomplauncher://game/<recomp id>--<id>`).
- `name`: shown on its tile.
- `args`: added after `start.args` when this game is played. Without them Play opens your own
  menu, and the tile says to pick the game there. They can carry this game's files with
  `{input:id}`, as `start.args` can; an argument whose file isn't chosen is left out:

  ```json
  { "id": "blue", "name": "Pokémon Blue", "args": ["--game=blue", "--rom={input:rom-blue}"], "inputs": ["rom-blue"] }
  ```
- `title_id` (Xbox 360): this game's title id. Only its own unlock file, `<title_id>.toml`, is read.
- `inputs`: the ids of the setup inputs this game needs. Inputs no game lists are shared by all.
  Easy Setup for a game asks only for its own and the shared ones.
- `saves`: where this game keeps its own saves, like the top-level `saves`. Without it, the
  release's saves are shared by its games.

With one game or none, the release shows as one game, as before. When your release has no list,
the one on recomplauncher.com is used. Before anyone installs your release, the launcher only has
that list to go on: ask for your games to be listed there (with their art), or the release shows
as one game until it's installed. Launchers older than 0.90 Beta 36 skip it and show one tile.

## Settings: in the launcher

### `config`: your settings files

Each entry is a file the launcher may edit before every start.

- `id`: a name settings use to point at it. Defaults to `"main"`.
- `format`: `"toml"`, `"ini"` or `"json"`. TOML and INI are edited line by line, so comments,
  order and keys the launcher doesn't know about stay as they are.
- `file`: where it is (see Paths).
- `game_path`: the key that holds the path to the player's copy. Written on every start.
- `ensure`: keys set only when they're missing or empty.
- `always`: keys set on every start. Keep this to what your recomp needs to run from the
  launcher.
- `new`: keys set only when the file doesn't exist yet. These are your defaults.

Values are written exactly as given, so TOML strings keep their quotes: `"\"xenos\""`.

### `settings`: what the player can change

Each setting is a row with a list of choices. On a controller, left and right move through them.

```json
{
  "id": "resolution",
  "label": "Resolution",
  "help": "What the game draws at. 720p is the console's own.",
  "choices": [
    { "label": "720p", "set": { "resolution_scale": "1" } },
    { "label": "1440p", "set": { "resolution_scale": "2" } }
  ],
  "defaults": { "resolution_scale": "1" }
}
```

- **Settings in a file.** A choice's `set` lists the keys it writes, in the file named by `in`
  (default `"main"`). The launcher shows the first choice whose keys all match the file. Keys
  the file doesn't have yet are read from `defaults`, so put your game's own defaults there.
- **Settings on the command line.** Set `"in": "args"` and give each choice `args` instead of
  `set`. The launcher remembers the pick and adds those arguments on Play. An empty `args` makes
  a good first choice, labelled "Game's option" or similar.
- **Settings that only apply sometimes.** `when` greys a setting out unless the keys it lists
  match. FIFA's Window size has `"when": { "fullscreen": "false" }`.

**Groups.** Give settings a `group` and a small heading shows where the group changes. Settings
keep the order you list them in.

```json
{ "id": "invert", "label": "Invert Camera", "group": "Controls", "choices": [ ... ] }
```

**Advanced options.** `"advanced": true` hides a setting until the player presses Show Advanced
Options, at the bottom of the list (the launcher remembers it per game). To hide a whole group,
list it next to `settings`. Hidden settings keep their values and are still written.

```json
"groups": [ { "id": "Advanced", "advanced": true } ]
```

**Sliders.** For a number, use `slider` instead of `choices`. Its `key` is read and written like
`set` keys (`Section/Key` works too), and `format` says how the number is written (printf style,
`"%g"` when you leave it out). A value off the step, like 1.37 set in the game's own menu, shows
as it is and stays until the player moves the slider. Left and right move one step.

```json
{
  "id": "sens", "label": "Mouse Sensitivity", "group": "Controls",
  "slider": { "key": "mouse_sensitivity", "min": 0.1, "max": 5, "step": 0.05, "format": "%.2f" },
  "defaults": { "mouse_sensitivity": "1.0" }
}
```

On the command line, use `"in": "args"` with `args` and `{value}` where the number goes:
`"args": ["--sens", "{value}"]`. It's passed once the player has moved the slider.

**Key bindings.** For controls kept one action per line (`key.cross=Space`), use `bindings`. Each
action's `key` is read and written like `set` keys. The game page lists the actions with their
keys as chips: the player presses a chip (or + for another key, up to `max`, 2 by default) and
then the key, mouse button or wheel. Several keys on one action are joined with `separator`.
Only actions the player changes are written, and Reset to Default puts back your `defaults`.

```json
{
  "id": "keys", "label": "Keyboard & Mouse", "group": "Controls",
  "bindings": {
    "separator": "|", "max": 2,
    "actions": [
      { "label": "Cross", "key": "key.cross" },
      { "label": "Options", "key": "key.options" }
    ]
  },
  "defaults": { "key.cross": "Space", "key.options": "Escape|Return" }
}
```

Keys are written as letters, digits, F1 to F12, Space, Return, Escape, Tab, Backspace, Shift,
Ctrl, Alt, Up, Down, Left, Right, Mouse Left, Mouse Right, Mouse Middle, Mouse 4, Mouse 5, Wheel
Up and Wheel Down. When your file already uses another name for one of them (Enter for Return),
that one is kept. To choose your own, add `"names": { "Return": "Enter" }`. `"pad": true` on an
action lets it take a controller button too (A, B, X, Y or Cross, Circle, Square, Triangle, LB,
RB, LT, RT, Start, Back, the sticks and the D-pad).

When the game closes, the launcher reads its settings again, so changes made in the game's own
menu show on its page. A value that isn't one of a setting's choices shows as it is.

Keep labels short and help text to a sentence. Say what it does for the player, not what the
key is called.

### Upscalers and DLSS

If your recomp has DLSS, FSR, XeSS or frame generation, expose it as a setting like any other.
These need data from inside the game for every frame (motion vectors, depth), so the launcher
can't add them from outside. It can give players one clear place to turn them on.

```json
{
  "id": "upscaler",
  "label": "Upscaling",
  "help": "DLSS needs an NVIDIA RTX card. FSR works on any card.",
  "choices": [
    { "label": "Off", "set": { "upscaler": "\"none\"" } },
    { "label": "FSR", "set": { "upscaler": "\"fsr\"" } },
    { "label": "DLSS", "set": { "upscaler": "\"dlss\"" } }
  ],
  "defaults": { "upscaler": "\"none\"" }
}
```

DLSS comes from NVIDIA's SDK, through Streamline or NGX directly. It isn't open source, so most
recomps can't ship it. The setting above only turns it on: to get the SDK to the player, see
**Vendor SDKs the build needs** under `prepare`.

### Looks

Players can pick a look for any game on Windows: Sharp, Vivid, Film, CRT or Studio monitor. They
run on ReShade, which the launcher downloads from reshade.me the first time. When the game starts
with a look, the launcher puts ReShade next to your executable as `dxgi.dll` (or adds it as a
Vulkan layer for that one process) and takes it away again when the game closes. ReShade's own
settings and log stay in the launcher's folder. Your release isn't changed.

You don't need to do anything for this. The launcher works out the graphics API from your
executable. Add `looks` only if one of these is true:

```json
"looks": {
  "graphics_key": "graphics_api",
  "in": "main"
}
```

- Your recomp can draw with more than one API and the player picks it in your settings file. Name
  that key in `graphics_key`. Values containing `vulkan`, `d3d12`, `d3d11`, `d3d9` or `opengl` are
  understood.
- It always uses one API the launcher might not spot, such as OpenGL through a library that's
  loaded at run time. Set `"graphics": "opengl"`.
- The CRT looks use the console's usual line count (240 for the Nintendo 64, 480 for the PlayStation
  2). If your game ran at a different one, set `"lines"`.
- Your recomp has its own filters, or ReShade doesn't work with it. Set `"allowed": false` and a
  `"reason"` to show the player.

The looks are made for ordinary (SDR) pictures. When a game outputs HDR they leave the picture
alone.

### `mods`

The game's page has a Mods tab when the launcher knows where your mods live. It lists the most
popular ones, installs them with a click and takes them out again. Thunderstore and GameBanana are
supported. Zelda 64: Recompiled, Banjo: Recompiled, Unleashed Recompiled, Marathon Recompiled and
the Ship of Harkinian family are built in, so leave this out for those.

```json
"mods": {
  "thunderstore": "your-community",
  "data": "YourRecomp",
  "gamebanana": 12345,
  "loader": "nrm"
}
```

- `thunderstore` and `data`: your community on thunderstore.io and the name of your data folder
  (in `AppData\Local` on Windows, `~/.config` on Linux). Mods go in its `mods` folder, or in the
  game's own `mods` folder when `portable.txt` is next to it.
- `gamebanana` and `loader`: your game's id from its GameBanana address and how your game reads
  mods. `nrm` puts `.nrm` files in the same mods folder as above (it needs `data`). `otr` puts
  `.otr` and `.o2r` files in a `mods` folder next to your executable. `hmm` is for Hedge Mod Manager
  mods with a `mod.ini`: they're copied next to your executable and switched on in `ModsDB.ini`, and
  `user` names your folder in `AppData\Roaming` that holds `cpkredir.ini`.

Only files that passed GameBanana's virus scan, and come as a zip, 7z or rar, can be installed from
the launcher. For the rest, the player gets a link to the mod's page.

### `network`: playing with others

If your recomp can be played online or on a local network, say so, and the game's page shows it:

```json
"network": {
  "modes": ["online", "lan"],
  "server": {
    "program": ["GameServer.exe"],
    "args": ["--port", "{port}", "--config={install}/server.ini"],
    "port": 27015,
    "label": "Dedicated server"
  }
}
```

- `modes`: `online` (over the internet), `lan` (on the same local network) or both.
- `server`: the dedicated server in your release, if it has one. The game's page gets Host a
  Server: it starts your program with `{port}` set to the port the player picks (`port` unless
  they change it), shows the address other players on the network connect to, and Stop ends it.
  Closing the launcher closes it too. What it prints goes to a log the player can open. A Windows
  server on Linux goes through Proton, like the game.

The launcher only starts your program: it doesn't relay traffic, open ports on the router or run
servers of its own. Leave `server` out when players connect to each other or to a server you run.

### `folders`, `actions` and `exit_codes`

- `folders`: named places the launcher can open or clear. When your game crashes, the newest
  `.log` in `logs` (or in your game's folder) is shown and goes with the report the player can
  send.
- `actions`: buttons under the settings. `open` opens one of your folders. `clear` empties one,
  written as a folder name and a subfolder (`"saves/cache"`). The launcher asks first.
- `exit_codes`: when your game exits with one of these, the player sees your message instead of
  a crash report. `"ok"` instead of a message: that's how your game closes when it's quit
  normally, so it isn't shown as a crash.

```json
"exit_codes": { "252968961": "It couldn't find your copy of the game. Pick it again on the game's page.", "1": "ok" }
```

## Preparation and verification

### `prepare`: your own setup step

If your own launcher does work before the first start, like pulling fonts or menu art from the
disc or building a cache, give the launcher a way to run it without a window:

```json
"prepare": {
  "program": ["yourgame.exe"],
  "args": ["--prepare", "--game", "{game}"],
  "label": "Menu art and fonts from your disc",
  "timeout": 300
}
```

It runs on the setup screen as "Getting the extras", after the copy is checked and before the
settings are written. It runs again whenever the player changes their copy.

- Don't open a window.
- Print lines like `progress 0.4 Unpacking menu art` as you go. The number is from 0 to 1 and the
  text after it shows under the step.
- Exit with 0 when it worked. Anything else shows as a failed setup, with your exit code.
- Write what you make to the same place your launcher would, so both stay in step.
- `"version"` (optional): change it only when players who already set up must run the step again,
  like when your output format changes. Nothing else in this file makes them set up again. Their
  next Play opens Easy Setup with a note that the game is built again, whether it's set up from
  named `inputs` or from a single `game` copy. A build the player pointed the launcher at
  themselves ("Use a Build You Already Have") isn't asked.
- To build into the player's install folder instead of their home folder, pass it to your builder
  (`"args": ["--cli", "--prepare", "--game", "{game}", "--out", "{install}/YourGame"]`) and start
  from the same place (`"start": {"args": ["--play", "{install}/YourGame"]}`, and `folders.game`).
  When that folder is gone, Play builds the game again, as it does for a folder under `{home}`.
- `"done"` (optional): the files the step leaves when it has finished, like
  `["{home}/Games/YourGame/prepared.bin"]`. When they're all there, the game counts as set up, even
  when your own launcher built it, and plays without asking for the copy (unless `start` uses
  `{game}`). If one goes missing, the player is asked to set up again. It's also what a finished
  build has: when a player points the launcher at a build of their own, the files of `done` in
  your build folder must be in theirs, so list one your
  build always leaves, like `["{install}/YourGame/game.pak"]`. Without `done`, any folder with files
  in it is taken.
- `"keeps_copy": true` (optional): your step keeps what it needs from the copy in your build folder
  (the first `folders.game`) and can run again from there without it, like an update that rebuilds
  from what the first build unpacked. Then a player who deleted their copy to save space isn't
  asked for it again: when `version` changes, or a file of `done` is missing, Play builds the game
  again from what you kept, and Easy Setup runs the step the same way when no copy is picked. The
  launcher leaves out every argument with `{game}` in it, so write the copy as one argument
  (`--game={game}`, not `"--game", "{game}"`); option args (`--fetch-ngx-sdk`) are still added. List
  `done` too, so the launcher can tell when the build is finished. When your build folder is gone,
  or the run fails, the player is asked for their copy and the game is built from that.

  ```json
  "prepare": {
    "program": ["YourGameBuilder.exe"],
    "args": ["--cli", "--prepare", "--game={game}", "--out", "{install}/YourGame"],
    "done": ["{install}/YourGame/yourgame.exe"],
    "keeps_copy": true
  },
  "folders": { "game": [{ "path": "{install}/YourGame" }] }
  ```

**A second file from the player.** When your step needs more than the copy, like an official patch
or an update, ask for it as a named input and pass it with `{input:<id>}`, next to `{game}`. The
setup screen asks for both. Don't download it in the step: players bring their own files.

```json
"inputs": [
  { "id": "patch", "label": "The official 2.0 patch (patch20.zip)", "role": "update",
    "formats": [{ "type": "file", "extensions": ["zip"] }] }
],
"prepare": { "program": ["yourgame.exe"], "args": ["--prepare", "--game", "{game}", "--patch", "{input:patch}"] }
```

[SETUP-INPUTS.md](SETUP-INPUTS.md) has the rest: several discs, optional DLC, and `{input:<id>}` in
`start.args` and `start.env` too.

#### `prepare.options`: ticks for a vendor's SDK

Some builds need a vendor's SDK at build time, like NVIDIA's DLSS SDK: it's not open source, you
can't ship it, and the shared `nvidia.dlss` input (a file the game loads) isn't enough because the
renderer is compiled with it. Give `prepare` an option. The setup screen shows it as a tick, and
when it's on its `args` are added after the step's own:

```json
"prepare": {
  "program": ["LongshotBuilder.exe"],
  "args": ["--cli", "--prepare", "--game", "{game}"],
  "options": [
    {
      "id": "dlss",
      "label": "NVIDIA DLSS and frame generation",
      "note": "Downloads NVIDIA's DLSS SDK during the build.",
      "gpu": "nvidia",
      "os": ["windows", "linux"],
      "args": ["--fetch-ngx-sdk", "--accept-nvidia-license"],
      "license": { "name": "NVIDIA RTX SDKs license", "url": "https://github.com/NVIDIA/DLSS/blob/main/LICENSE.txt" }
    }
  ]
}
```

- `id` (a-z, 0-9, `_`), `label` and `args` are required. `note` is a line under the label.
- `os` (`windows`, `linux`, `macos`) and `gpu` (`nvidia`, meaning an RTX card, or `amd`) decide who
  sees it. Leave them out to show it everywhere.
- With a `license` (a `name` and an `https` `url`) the tick starts off, and ticking it is the
  player accepting it. Your args are what pass that on, like `--accept-nvidia-license`: never accept
  for the player yourself. Without one it starts on, or as `default` says.
- `"required": true` is for something the build can't run without. The setup screen's buttons
  stay off, with a line saying why, until it's ticked, and the launcher refuses to start the step
  without it. Only the PCs it's shown on count: an option `os` or `gpu` hides is never required
  there. With a `license` it still starts off: only the player accepts it. Launchers older than
  0.90.0-beta.64 don't know `required` and show a plain tick, so keep your builder's own check
  (exit with a code and say why) until your `min` is that version:

  ```json
  "options": [
    {
      "id": "vs_build_tools",
      "label": "Microsoft Visual Studio Build Tools",
      "note": "Needed to build the game on your PC: installed if this PC doesn't have them.",
      "os": ["windows"],
      "required": true,
      "args": ["--accept-vs-license"],
      "license": { "name": "Microsoft Visual Studio license terms", "url": "https://visualstudio.microsoft.com/license-terms/" }
    }
  ]
  ```
- Your builder gets only these args. The launcher sets no environment variables for it, so keep
  the SDK in your builder's own folder and put runtime files where the game looks.
- Print `progress` lines for the download too. If it fails, exit with a code other than 0: the
  player sees "Its setup step stopped with code N", so put the reason in your own log.
- An option you add in a later release reaches players who already set the game up: Set Up
  Again, beside Play, gets a New mark, and the next time they press Play they're asked once to turn
  it on (the setup runs again with the copy they set up with) or play as it is. Their answer is
  kept, so make the step cheap to run again: reuse what the last run made.

#### Vendor SDKs the build needs

Some features can't be built without a vendor's SDK, like NVIDIA's DLSS (NGX). You can't ship it,
and the launcher can't add it from outside. Have your builder fetch it on the player's PC, after
the player agrees to the vendor's license:

1. Give the builder a flag that downloads the SDK from the vendor, and a second flag that says
   the license was accepted. Without the second, it downloads nothing and stops.
2. Add a `prepare.options` tick, as above, with both flags in `args`, a `license`, and `gpu`
   (`nvidia` for DLSS). Ticking it is the player accepting the license.
3. Keep the SDK in a folder of your builder's own. The launcher sets no environment variables for
   `prepare`, so the flags must work without any: a variable you read today can still override the
   folder, but it can't be required. Check it by running the builder exactly as the launcher does:
   the arguments above, nothing else set.
4. Put the vendor's runtime files where the game looks for them, never in your release. For
   DLSS, that is `nvngx_dlss.dll`, and `nvngx_dlssg.dll` for frame generation, in an `ngx` folder
   in the game's folder.
5. With the tick off, the build must be the one you ship today.
6. Print `progress` lines while it downloads. If it fails, exit with a code other than 0 and
   write the reason in your own log: the player sees "Its setup step stopped with code N".

### `game.dumps`: known good copies

```json
"game": {
  "kind": "iso",
  "check": { "xdvdfs": "default.xex" },
  "dumps": [
    { "name": "USA", "size": 7838695424, "crc32": "1a2b3c4d" },
    { "name": "Europe", "size": 7838695424, "crc32": "5e6f7a8b" }
  ]
}
```

The launcher reads every byte of the player's copy anyway, to make sure it's complete. With
`dumps` it also compares the CRC32 and shows "Matches the verified USA disc". Take the values
from Redump. A copy that doesn't match still works; the player just doesn't get the badge.

## Controllers and Big Picture mode

Big Picture mode is the launcher's full-screen view for a controller, the couch and the Steam Deck.
Players pick a game, set it up and press Play without touching a mouse, so the best thing your
recomp can do is carry on the same way.

- **Use SDL3 for input if you can.** SDL3's gamepad support knows hundreds of controllers (Xbox,
  DualSense and DualShock, Switch Pro, 8BitDo, the Steam Deck) and names their buttons the same
  way, with gyro, touchpads, rumble and lights where the pad has them. Recomps on SDL2 work fine;
  SDL3 is where controller support is going, and it's what we'll build further features on.
- **Be playable start to finish on a controller.** Menus, settings, the "pick your copy" step if
  you still have one, and quitting. A player in Big Picture mode may have no keyboard in reach.
- **Quit cleanly.** When your game closes, the launcher backs up the save straight away. A "Quit"
  in your menu means a controller player never needs a keyboard to get back to their library.
- **Read `RECOMPLAUNCHER_MODE`.** Starting full screen in `bigpicture` and windowed in `desktop` is
  what most players expect.
- **Use `{pad}` or `RECOMPLAUNCHER_PAD`** to default your button prompts to the player's
  controller.
- **Leave background input to the player.** If your game reads controllers while it's not in
  front (SDL's `SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS`), keep it off by default: the launcher's own
  overlay uses the controller too.

## Make it run its best

Things that make a recomp feel finished in the launcher. None are required; each one shows.

- **Good defaults.** Start at the console's own look (resolution, frame rate, aspect) and let
  players go up from there in your settings. A first start that just works beats one that asks.
- **Settings the launcher can show.** Keep your options in a TOML, INI or JSON file and describe the
  ones players care about in `settings`: resolution, frame rate, anti-aliasing, V-Sync, window mode,
  upscaling. They show on the game's page and in Big Picture mode, with your help text.
- **High frame rates done right.** If you unlock the frame rate, say in the help text what it
  changes (some games tie physics or cutscenes to it) and keep the console's rate as an option.
- **Say where saves go** with `saves`, so backups and cloud saves pick up exactly the right files.
- **Write a log.** A `.log` in a `logs` folder (or next to your program) goes with the crash
  report a player sends, so you get it without asking.
- **Explain your exit codes** with `exit_codes`. A clear "Your copy is the European release; this
  needs the USA one" saves you a bug report.
- **Support `portable.txt`.** If a `portable.txt` next to your program makes your recomp keep
  everything in its own folder, mod installs from the launcher follow it.
- **Check the copy, then trust it.** With `game` and `dumps`, the launcher has already checked the
  player's copy before your program starts, so your own check can be quick.
- **Ship a `recomplauncher.json` in every release**, and keep it in step with your settings when
  you add or rename keys. The Developer tab (below) shows the problems before players do.

## Launcher features

A profile can list the launcher features it uses in `"requires"`, and the oldest launcher it works
with in `"min"` (like `"0.90.0-beta.27"`). A launcher that doesn't have one of them leaves out the
parts that need it (no Easy Setup, no settings tab for it), and the game still installs and plays the
normal way. It never fails on a profile it doesn't fully understand.

RecompLauncher 0.90 Beta 27 supports:

| Feature | What it is |
| --- | --- |
| `start` | The program to start |
| `start.linux` | A separate program on Linux |
| `start.macos` | A separate program on a Mac |
| `setup` | A program in the release that runs once before the first play |
| `console` | The program asks questions in its text window |
| `clone` | The game's setup works from a clone of its repo |
| `makes` | Files or folders its setup makes, to tell when it finished |
| `player_note` | Something the player does by hand, shown on the game's page |
| `installer` | The release is an installer, then the game starts from what it installed |
| `needs` | What the PC needs for its setup: `build_tools`, `clang`, `python` (3.10 or newer), checked on the game's page |
| `exit_codes` | What a game's exit codes mean (`{"70": "…"}`, or `"ok"` for a normal quit), shown in the crash window |
| `files.folder` | Easy Setup: the player's copy goes into a folder |
| `files.flag` | Easy Setup: the copy is passed on the command line |
| `files.env` | Easy Setup: the copy reaches the game through an environment variable |
| `files.setup` | Easy Setup: the copy is handed to the game's own setup |
| `files.accepts` | Which kinds of copy it takes (a disc image, a folder, a file) |
| `files.contains` | Files the copy must have, to check it's the right game |
| `files.sha256` | Known dumps, checked by their SHA-256 |
| `files.update` | A title update the game needs beside the disc |
| `settings.toml` | Game settings in a TOML file |
| `settings.ini` | Game settings in an INI file |
| `settings.json` | Game settings in a JSON file |
| `settings.args` | Game settings on the command line |
| `settings.choices` | Settings with a list of choices |
| `settings.slider` | Settings as a slider |
| `settings.bindings` | Key and button bindings |
| `settings.groups` | Settings in named groups |

## Betas

Publish a test build as a GitHub pre-release. Players on your normal release never get it by
accident: the launcher always installs your latest full release. When you want players to try the
pre-release, tell us and we open it as a beta. It can be open to everyone, by request, or by
invite. Players join from the launcher or their account page, play it, and go back to your normal
release with one button. You can also name one release by its tag instead of "the newest
pre-release".

## Recomp Standards

What makes a recomp easy to play, wherever players get it from. Nothing here is required to be listed on RecompLauncher. These are the things players trip over most, and most of them are small changes.

### The basics

**Never ship game files.** No ROMs, disc images, extracted assets or keys. Players bring their own copy.

**Accept the formats people actually have.** Most backups are packaged files, not the extracted folders a recomp is often first written for. Support the standard packaged formats where they apply:

| Console | Accept |
| --- | --- |
| PS1 | CHD, BIN/CUE, ISO |
| PS2 | CHD, ISO |
| PSP | CHD, ISO, CSO |
| PS3 | a decrypted ISO, or the game folder (PS3_GAME) |
| PS4 | the dumped game folder (CUSA…), or a fake-signed PKG |
| PS5 | the dumped game folder (PPSA…) |
| PS Vita | a decrypted dump: VPK or the game folder (PCSA…, PCSE…) |
| Dreamcast | CHD, GDI |
| Saturn, Sega CD / Mega CD | CHD, BIN/CUE |
| TurboGrafx-CD / PC Engine CD, Neo Geo CD | CHD, BIN/CUE |
| 3DO, Amiga CD32, Philips CD-i | CHD, BIN/CUE |
| GameCube, Wii | RVZ, ISO, GCZ (and WBFS for Wii) |
| N64 | .z64, .n64 and .v64 (any byte order), zipped |
| NES, SNES, GB, GBA, DS | the ROM, or zipped |
| Switch, Switch 2 | NSP, XCI and NUP, not only extracted exefs/romfs (with the player's keys, see below) |
| Xbox | ISO (XISO), or an extracted folder |
| Xbox 360 | ISO, or an extracted folder with default.xex |

CHD is the format most disc-based backups come in, and the one players ask for most. Reading it is a small library ([libchdr](https://github.com/rtissera/libchdr), BSD licensed).

**Make the player's own copy and keys easy to use.** Where a decrypted dump exists (a decrypted PS3 ISO, a dumped PS4 or PS5 folder, a decrypted Vita game), accept it as is, so players don't have to run a separate tool first. Where the game has to be decrypted (Switch NSP, XCI and NUP, for example), read the player's own keys from a file they point to, with an option like `--keys <path>`. Then a launcher can pass the keys file the player added (RecompLauncher can), instead of the player copying it into your folder. Never ship keys yourself.

**Check the copy, then say what's wrong.** If you know the good dumps (from [Redump](http://redump.org) or [No-Intro](https://no-intro.org)), check against them. When a file is wrong, say why in one line ("This is the PAL version. The recomp needs the USA one.") instead of crashing later.

**Let the game's location be passed in.** A `--game <path>` option (a file or a folder) means launchers, scripts and Steam shortcuts can start your recomp without anyone moving files around. Keep your own picker for everyone else.

### Releases

- **Tag every release with a version** (`v1.2.0`) and attach the builds to the GitHub release, not just to the README or Actions.
- **One file per platform, with a name that says what it is**, the same way every time: `yourgame-1.2.0-windows-x64.zip`, `-linux-x86_64.AppImage`, `-macos-arm64.zip`.
- **Build for Linux and Mac too** when you can. A lot of players are on Steam Deck.
- **Say what changed** in the release notes, even in one line.

### On the player's PC

- **Portable when it can be.** If the folder next to the exe is writable, keep saves, settings and logs there. Otherwise use the usual per-user folder, and say where in the README.
- **Settings in a file a person can read**: TOML, INI or JSON, with every option written out and commented. Don't hide settings in the registry.
- **Logs in a known place**, kept for the last few runs, so players can send them when something breaks.
- **Controllers through SDL**, with buttons that can be remapped and a sensible default for Xbox, PlayStation and Switch pads.
- **No admin rights, no installer required.** Unzip and play.
- **Don't download anything at first start** without asking.

### Extras that players notice

- **recomplauncher.json (this guide)**: one-click setup and your settings on the game's page in RecompLauncher.
- **[Recomp Achievements](https://github.com/elripalda/RecompAchievements)**: achievements, Beaten and Mastered, leaderboards.
- **A "Get it on RecompLauncher" badge** in your README, from the [press kit](https://recomplauncher.com/press).
- **A short "How to play" at the top of the README**: what copy of the game you need, and where to put it.

## Get listed

Recomps on recomplauncher.com are in the launcher. If yours isn't there yet, [submit it](https://recomplauncher.com/submit?platform=pc)
with its GitHub repo, and say which parts it supports. A free [developer account](https://recomplauncher.com/developers?signup=1)
gets it reviewed faster. Already listed and adding `recomplauncher.json`? Nothing to do: the next
release you publish carries it.

## Checking your file

The launcher has tools for this. In Settings, open Developers and turn on Developer tools.

1. **Add your build.** Use "Add a build" and pick your recomp's folder, the one with your program
   and `recomplauncher.json`. It shows in your Library as a local build. Nothing is downloaded, and
   the launcher never updates, moves or deletes that folder.
2. **Read the check.** The build opens on its Developer tab. It reads your file fresh each time
   and lists what it found: where Play starts, what the setup screen asks for, each settings file
   and the choice each setting shows right now. Problems are marked in red, things worth a look in
   yellow. JSON mistakes show the line and column. It also sums up how the game is set up
   (launches with the player's copy, or needs a one-time step) and what it can do.
   No file yet? "Make a starter file" writes one next to your program to begin from.
3. **Try it like a player.** Set it up with your own disc, change a setting, then open your
   settings file: only the keys you listed should have changed. Press Play: your game should start
   without your own launcher.
4. **Change the file and press Check again.** No restart needed.

Add `"name"` to your file and the local build goes by that name.

If you'd rather check a release the way players get it, install it from Discover and open its
page. A file the launcher can't read is skipped, and the built-in one (if any) is used instead.

Stuck, or want us to write the file with you? Open an issue, or send us your file.

## Achievements

Recomp Achievements is RecompLauncher's achievement system: achievements pop up as players earn them, whole games can be Mastered, and Ranked points go on the leaderboards. It's separate from `recomplauncher.json`: a small C library in your recomp reports unlocks to the launcher, using the game's own achievements (Xbox 360), rules on console memory, or events you fire from your code.

Everything you need is in the [Recomp Achievements repository](https://github.com/elripalda/RecompAchievements). Xbox 360 recomps on a recent ReXGlue build may already work without any change.
