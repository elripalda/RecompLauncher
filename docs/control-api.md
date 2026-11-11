# Local control

Scripts and AI tools on the same computer can see which games are installed and running, and start
one with Play. That's all: installing, removing, updating and changing settings stay in the
launcher's window.

It's off until you turn it on: **Settings > General > Local control**, or in Big Picture mode,
**Settings > Local control**. Turning it off stops it at once.

## How it's kept to this computer and this user

- It listens on `127.0.0.1` only, on a new random port each time it starts.
- Every request needs a bearer token, a new random one each time it starts.
- The port and token are written to `control-api/endpoint.json` in the launcher's data folder,
  readable by you only. The folder and the file must belong to you, and they're checked through
  the handle the token is written with, while the file is still empty:
  - macOS and Linux: the folder is `0700` and the file `0600`. On macOS any access list entries
    (`ls -le` shows them) are taken off too, including ones a folder above hands down.
  - Windows: the folder and the file each have a protected access list (nothing inherited from
    the folders above) with one entry, for you, and you as their owner. The folder gets that list
    as it's made. A `control-api` folder that was already there without it is refused rather than
    repaired: delete it and turn Local control on again.
- No other user may be able to rename or replace the folders it's in, or a link on the way to it
  (they could put their own endpoint file in its place). Every folder and link is checked as the
  path is named, from the top, not only where it leads. Where another user could, it doesn't
  start, and Settings says why. That refuses:
  - a folder on the way that belongs to someone other than you or root (Administrators, SYSTEM
    or TrustedInstaller on Windows);
  - a folder anyone may change, or its group may change, unless it's sticky like `/tmp`. That
    includes your own group: on Linux, a folder made with umask `002` is group-writable, so
    `chmod g-w` it. (On Linux, an access list entry letting another user write also shows up as
    group write.)
  - macOS and Linux: a link in a folder others may change, or one that belongs to another user,
    wherever it points; and a link whose target goes through such a folder. A link of yours or
    root's in safe folders is fine, so macOS's `/var` and `/tmp` (root's links to `/private`)
    work. More than 40 links on the way is refused, which ends a loop of links.
  - Windows: any link, junction or mounted folder on the way, even a safe one (whether others
    could change where it leads isn't worked out). So a data folder reached through a junction
    (AppData moved to another drive that way, say) is refused.
  - a drive that can't keep access lists (FAT32, exFAT, some network drives), a portable copy on a
    USB stick, or one in a folder made at the top of a Windows drive (`C:\Games`, say, which
    every signed-in user may change).
  - Flatpak: folders the sandbox shows as owned by `nobody` (the host's own folders) are refused
    like any other user's, so Local control may not start in the Flatpak. This hasn't been tried.
- Requests from web pages are refused: the `Host` must be `127.0.0.1:<port>` or
  `localhost:<port>`, any `Origin` or `Sec-Fetch-*` header is refused, and POST bodies must be
  `application/json`. Sizes, connections and waits are limited.
- The token is never written to the launcher's log.

Anyone who can already run programs as you could read the file and use it. That's the same trust
a desktop shortcut gets. Administrators (root) can read anything, as always.

## Where the endpoint file is

| | |
| --- | --- |
| Windows | `%APPDATA%\com.recomplauncher.app\control-api\endpoint.json` |
| macOS | `~/Library/Application Support/com.recomplauncher.app/control-api/endpoint.json` |
| Linux | `~/.local/share/com.recomplauncher.app/control-api/endpoint.json` |
| Portable | `data\control-api\endpoint.json` beside the launcher |
| Flatpak | `~/.var/app/<app id>/data/com.recomplauncher.app/control-api/endpoint.json` |

Settings shows the exact path while it's on.

```json
{
  "schema": "recomplauncher-control-api/1",
  "api_version": "1",
  "url": "http://127.0.0.1:53124",
  "mcp_url": "http://127.0.0.1:53124/mcp",
  "port": 53124,
  "token": "…64 hex characters…",
  "pid": 4242,
  "app_version": "0.90.0-beta.36"
}
```

Read it again whenever a request fails: the port and token change each time the launcher starts.

## HTTP

Send `Authorization: Bearer <token>` with every request, and `Content-Type: application/json`
with a POST.

### `GET /v1/status`

```json
{
  "ok": true,
  "api_version": "1",
  "app": {"name": "RecompLauncher", "version": "0.90.0-beta.36"},
  "games": [
    {"id": "zelda64recomp", "name": "Zelda 64: Recompiled", "version": "v1.2.0", "running": true, "missing": false},
    {"id": "cogbox", "name": "Collection", "version": "v3", "running": false, "missing": false,
     "games": [{"id": "cogbox--one", "name": "One", "running": false}]}
  ],
  "running": ["zelda64recomp"]
}
```

`missing`: the game's folder isn't there right now (a drive that isn't connected). `games`: the
games inside a recomp that carries several; Play takes their ids too.

### `POST /v1/play`

The body is exactly `{"id": "<game id>"}`. Other fields are refused.

The game starts the way its desktop shortcut starts it, with its shader and its cloud save.
Other running games keep running. Play never installs a game, runs a game's setup or installer,
installs its Flatpak, or has Windows ask for administrator: a game that needs any of that is
refused with `needs_setup`, and you do it in the launcher's window. Getting the game ready can
still download the shader you picked for it, and, in the Flatpak, unpack an AppImage the first
time, as its shortcut does.

When the game (or another game of its recomp) is running or starting already, Play answers so
before anything is done: no shader download, no cloud save sync.

```json
{"ok": true, "id": "zelda64recomp", "recomp": "zelda64recomp", "result": "started", "started": true, "already_running": false}
```

`result` is what Play did:

| `result` | |
| --- | --- |
| `started` | It started now |
| `already_running` | It was running already; nothing more was done |
| `already_starting` | Another Play is starting it right now; nothing more was done |

| Status | `error.code` | |
| --- | --- | --- |
| 400 | `invalid_args`, `invalid_id` | Not `{"id": "…"}` |
| 404 | `not_installed` | No installed game has that id |
| 409 | `missing` | Its folder isn't there right now |
| 409 | `needs_setup` | It needs something done in the launcher's window first: its setup or installer, its Flatpak installed, administrator rights, or its game files picked. When it's the catalog's Setup Required, the window opens to do it |
| 409 | `busy` | Its recomp is playing (or starting) another of its games; that has to close first |
| 422 | `play_failed` | The launcher couldn't start it; `message` says why |
| 503 | `stopped` | Local control was turned off |

Every error is `{"ok": false, "error": {"code", "message"}}`. Any other path is 404.

```sh
EP=~/Library/Application\ Support/com.recomplauncher.app/control-api/endpoint.json
URL=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["url"])' "$EP")
TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$EP")
curl -s -H "Authorization: Bearer $TOKEN" "$URL/v1/status"
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d '{"id":"zelda64recomp"}' "$URL/v1/play"
```

## MCP

`POST /mcp` speaks MCP (JSON-RPC 2.0, one message per request) with two tools:

- `launcher_status`: the same as `GET /v1/status`. Read only.
- `play_game` `{"id"}`: the same as `POST /v1/play`.

There are no other tools, resources or prompts.

Most MCP clients start a program and talk to it over stdio. `tools/control-api/recomplauncher_mcp.py`
(Python 3.9 or newer, nothing to install) is that program. It reads the endpoint file each time,
sends nothing anywhere but `127.0.0.1`, and ignores proxy settings. On macOS and Linux it only
uses an endpoint file that is a plain file of yours that nobody else can open (as the launcher
writes it). On Windows it doesn't check: use the path Settings shows. For example, in a client's MCP
settings:

```json
{
  "mcpServers": {
    "recomplauncher": {"command": "python3", "args": ["/path/to/recomplauncher_mcp.py"]}
  }
}
```

A portable or Flatpak launcher: add `"--endpoint", "/path/to/endpoint.json"` to `args`.

## Turning it off

Turning it off closes the port, removes the endpoint file and refuses further starts. A Play that was
still getting ready (downloading the shader, syncing the cloud save) doesn't start its game, even
if Local control is turned on again. Games already running keep running.

Shader waits are cancelled. Cloud sync finishes with its save lock and game-start claim held, so
another Play cannot start the game while a blocking restore still writes saves. Other preparation
also may not be interruptible (unpacking a game the first time). Turning it off waits
up to five seconds for that. After that it's off all the same, and Settings says a Play is still
finishing getting ready, until it has. That Play never starts its game: the game's start itself
checks the switch, and turning it off waits for a start under way.

If the setting can't be saved, Settings says so. Turned on, it's left off then. Turned off, it
stays off, but it may come back on the next time the launcher starts.

## Not in this version

Installing, removing, updating, changing settings, the window and quitting aren't available. They
may come later, one at a time, once this has been reviewed.

## Tests

- `cargo test control` (in `src-tauri`): the server on a real loopback port with a stand-in
  launcher. It covers the token, the web-page refusals, every other path being absent, Play taking
  only an id, MCP offering only the two tools, the stdio adapter, private files on each system
  (macOS access list entries, folders and links others could replace, Windows owners and existing
  folders), that turning it off closes the port and cancels safe preparation waits, that a
  blocking save restore keeps its locks and start claim until it ends, that a Play stuck past
  the off switch's wait never starts its game even once it's on again, and the
  switch with a Library that can't be saved. Owners other than you and root are tested on the
  rule alone; no test runs as a second user. Play's own rules are tested on the launcher's own code:
  every setup, installer and administrator call on Play's way needs leave that only the player's
  Play gets, what Play answers comes from the start's own claim (a game running or starting
  already, or another game of the same recomp), and that claim comes before anything is got
  ready (a stand-in cloud restore writes nothing for those). No game or installer is run by the
  tests.
- `python3 tools/control-api/test_recomplauncher_mcp.py`: the stdio adapter.
- `node --test tests/control-api-settings.test.cjs`: the Settings switch, desktop and Big Picture.
