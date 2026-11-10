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

### AppImages

The launcher reads `/recomplauncher.json` directly from the AppImage's SquashFS payload.
It does not execute or mount the image to inspect it, and writes no extracted JSON sidecar.
Settings, save and folder paths still start from the installation directory (`{install}`),
not from the internal filesystem.

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
  - `iso9660`: a file in the top folder of a PlayStation 2 (or any ISO 9660) disc image.
  - `nup_build_id`: the build ID of a Nintendo Switch 2 `.nup` package, e.g. `"e0f3ccfa"` (hex,
    at least 8 digits; a prefix of the full 64-digit ID is fine). It is read from the `exefs/main`
    of the package's newest title, from the table rows leading to it and the first 96 bytes of that
    file only, never the whole package, and a folder with `exefs/main` is read the same way. The
    file is recognised by its contents, not its extension. This is a content check only: it needs
    and adds no keys, and nothing is copied. A wrong build is refused with the build it found. Use
    it with `kind: "iso_or_folder"`.

The same check is how the launcher finds copies in the player's folders, so a disc image named
`backup-07.iso` is still found.

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
| `{game}` | The player's copy (the .iso or the folder) |
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

Without it, the launcher watches which files the game writes while it runs, and uses those places.
That works for most games, but saying where is better. Only list saves and small settings: game
files, programs and archives are never copied, and nothing over 64 MB.

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

DLSS ships through NVIDIA's Streamline SDK, which is free to use. If you're adding it, the
setting above is all the launcher needs.

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

## Get listed

Recomps on recomplauncher.com are in the launcher. If yours isn't there yet, submit it from the
website (Submit a Game) with its GitHub repo. Already listed and adding `recomplauncher.json`?
Nothing to do: the next release you publish carries it.

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
