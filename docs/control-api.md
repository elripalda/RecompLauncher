# Local control API and MCP

RecompLauncher can be driven by scripts and AI agents without clicking its window: an HTTP API
inside the running app, on the loopback interface, and an MCP server (over that API, and over
stdio through a small adapter). It runs the launcher's own backend commands, the same ones the
window calls, with the app's live state.

It is **off unless turned on**, and runs on **macOS and Linux only** (on Windows it refuses to
start; see Discovery and security).

## Turning it on

Any one of these, checked when the launcher starts:

| How | Effect |
| --- | --- |
| `--control-api` on the command line | On for this run. Given to a launcher that's already running (through its single-instance hand-off), it turns the API on there without touching the window. |
| `<data dir>/control-api/enabled` exists (`recomplauncher_mcp.py enable`) | On at every start. |
| `RECOMPLAUNCHER_CONTROL_API=1` / `=0` | Forces it on / off (off wins over the two above, including `--control-api` handed to a launcher that's already running: what counts is the running launcher's environment). |
| `RECOMPLAUNCHER_CONTROL_PORT=<port>` | A fixed port instead of a random one (still 127.0.0.1 only). |

The data dir is `~/Library/Application Support/com.recomplauncher.app` on macOS,
`~/.local/share/com.recomplauncher.app` on Linux, or the `data` folder beside a portable
launcher.

Unattended start (no window clicks):

```sh
# macOS: start it with the API, in the background (or turn the API on in the running one)
python3 tools/control-api/recomplauncher_mcp.py launch
# same thing by hand
open -g -a RecompLauncher --args --control-api                       # not running yet
~/Applications/RecompLauncher.app/Contents/MacOS/recomplauncher --control-api   # running: hands it over and exits
# stop it
python3 tools/control-api/recomplauncher_mcp.py app quit             # or: recomplauncher --quit
```

## Discovery and security

When the API starts it writes `<data dir>/control-api/endpoint.json`:

```json
{ "schema": "recomplauncher-control-api/1", "url": "http://127.0.0.1:53817", "mcp_url": "http://127.0.0.1:53817/mcp",
  "port": 53817, "token": "<64 hex chars>", "pid": 12345, "app_version": "0.90.0-beta.12",
  "identifier": "com.recomplauncher.app", "data_dir": "...", "started_ms": 1793000000000 }
```

- Listens on **127.0.0.1 only**, random port; connections from anything but loopback are dropped.
- **Bearer token**: 32 random bytes from the OS, new each start, compared in constant time. It
  is in the endpoint file only (mode **0600**, folder **0700**). It is never logged; the
  adapter refuses an endpoint file other users can read and never prints the token.
- **macOS and Linux only.** On Windows the API does not start (the launcher logs why) and the
  adapter refuses to read an endpoint file: no private ACL is set on the file yet, and a
  portable data folder may sit on a shared drive, so the token could be read by other users.
- `RECOMPLAUNCHER_CONTROL_API=0` (or `off`/`false`/`no`) in the launcher's environment keeps
  the API off however it is asked for: the `--control-api` switch, the `enabled` file, or
  `--control-api` handed over by a second launch to the launcher already running.
- **Browsers are refused**: the `Host` header must be `127.0.0.1:<port>` or `localhost:<port>`
  (stops DNS rebinding); any `Origin` or `Sec-Fetch-*` header is refused (every request a web
  page makes carries one); POST bodies must be `application/json` (no simple-request CSRF);
  `OPTIONS` is refused and no CORS headers are ever sent.
- **Bounds**: 16 KiB request head, 1 MiB body, `Content-Length` only (no chunked), 10 s to
  send a request, 16 connections at once, 32 unfinished calls, waits capped (calls 10 min,
  events 30 s), 2000 events and 256 jobs kept.
- The file is removed when the launcher exits.

Commands run **inside the app process** through Tauri's own IPC pipeline: the API hands an
`InvokeRequest` to the main webview's `on_message` with the invoke key from
`AppHandle::invoke_key()` (Tauri's public accessor for backend invoke systems). The app's
`generate_handler!` dispatch, the capability check and the injection of `State`, `AppHandle` and
the window all happen exactly as for the window. The invoke key never leaves the process, and no
JavaScript is injected into the page.

## The command registry

`src-tauri/build/control_registry.rs` reads, at build time, the `tauri::generate_handler![...]`
list in `src-tauri/src/lib.rs` and each listed `#[tauri::command]` function: its name, doc
comment, injected context, parameters (as Tauri expects them: lowerCamelCase keys) with a JSON
Schema for each (structs like `Settings` and `Adopt` included), and return type. It also collects
every event name the backend emits. Nothing is listed by hand, so a command added to the handler
shows up in the API and MCP on the next build. Tests check the generated list against the source.

A few notes the source can't express live in `src-tauri/src/control/registry.rs` (and are
checked against the registry by tests): which commands only read, which report progress by
event, and how each cancellable one is cancelled.

## HTTP API

All requests: `Authorization: Bearer <token>`; POSTs `Content-Type: application/json`.
Errors are `{"ok": false, "error": {"code", "message", "details"?}}` with a matching status.

| Route | What |
| --- | --- |
| `GET /v1/status` | App (name, version, identifier, pid, data dir), API (commands, events, active jobs), main window, launcher (running games, background jobs, installed ids, setup done). |
| `GET /v1/commands?q=&module=` | Every command with a summary and its parameters (`name?` = optional). |
| `GET /v1/commands/{name}` | One command in full: doc, Rust path and signature, parameter schemas, `input_schema`, progress event, cancel. |
| `POST /v1/invoke` | `{command, args, async?, timeout_ms?, validate?}`. Answers `200 {ok, result, job_id}` when done, `202 {job}` if still going after `timeout_ms` (default 30000) or with `async: true`, or an error. snake_case argument keys are accepted. `validate: false` skips the API's own argument checks (Tauri's still apply). |
| `GET /v1/jobs`, `GET /v1/jobs/{id}?wait_ms=` | Calls as jobs: `running`, `cancelling`, `succeeded`, `failed`, `cancelled`, with result/error, latest progress and recent events. Secrets in arguments (keys, tokens, headers) are shown as `***`. |
| `POST /v1/jobs/{id}/cancel` | Cancels through the launcher's own cancel (`install` → `cancel`, `setup_copy` → `setup_cancel` with the job's run, `account_start` → `account_cancel`); other jobs answer `409 not_cancellable`. `200 {pending: false}`: sent. `202 {pending: true}`: the command hasn't answered yet, so the cancel waits and is sent the moment it does (the job is `cancelling` meanwhile). Either way the job ends with the work's closing event (`account_start`: as soon as the cancel is in). |
| `GET /v1/events?after=&names=a,b&limit=&wait_ms=` | The launcher's events, numbered; pass `next_after` back as `after`. `wait_ms` long-polls (max 30000). |
| `POST /v1/app` | `{action: status | show | hide | focus | quit}`. |
| `POST /v1/settings` | `{patch}`: merged into the current settings (RFC 7386: objects merge, `null` takes a setting back to its default) and saved by the launcher's `patch_settings` in one step under its own lock; answers with the settings as saved. |
| `GET /v1/limitations` | What isn't available and what to use instead. |
| `POST /mcp` | MCP JSON-RPC (below). |

Error codes: `unauthorized` (401), `bad_host`, `browser_request_forbidden` (403),
`unknown_command`, `job_not_found`, `not_found` (404), `invalid_args`, `invalid_json`,
`bad_request` (400), `command_failed` (422: the command's own error message), `not_cancellable`,
`already_finished`, `cancel_failed` (409), `payload_too_large` (413), `unsupported_media_type`
(415), `too_many_jobs` (429), `busy`, `unavailable` (503).

Jobs: every call is a job. Commands that hand work to the background and report it by event
(`install`, `setup_copy`) stay `running` after they answer, take progress from their events
(`install` / `setup`, matched by game id) and end with the closing event (`done` / `error` /
`cancelled`); an install that ends without one is reconciled against `get_state.jobs`.
`setup_copy` answers with a run number and every `setup` event carries it (`run`), so a job
follows only its own run: events that arrive before the answer are matched once it comes, and
each run ends with exactly one closing step — `done`, `error`, or `cancelled` when
`setup_cancel` stops it (from the window, a direct call, or the job's cancel) or a newer
`setup_copy` for the same game replaces it. A run that stops without saying how reports
`error`. The closing event comes from whichever side ended the run, so there is only ever one:
a run's last step (writing the game's settings, remembering the copy in the Library and saving
it) happens in one step against `setup_cancel` and a newer run, and a run cancelled or replaced
before it writes nothing and sends no `done`. The launcher's own setup dialogs (window and Big
Picture) follow only the run `setup_copy` answered them with, keep events that arrive before that
answer, and close when that run is cancelled or replaced from elsewhere.

Cancelling: a job's cancel is sent only once its command has answered. Before then the work may
not have started (`setup_copy` waits for the Library lock first, say), so a cancel sent at once
could find nothing to stop and the work would start after it; instead the job is `cancelling`
with the cancel waiting, and it goes in as soon as the command answers. `setup_copy`'s cancel is
bound to the job's run (`setup_cancel {id, run}`): it stops that run only, never a newer one
started since (from the window or another call). `setup_cancel` without `run` (the window's old
call) stops whichever run is going; it answers whether it stopped one. Others
(`move_game`, `gb_install`, `ge_proton_install`, `plugin_install_zip`, `update_launcher`, `play`)
end when the command answers and show their events as progress.

After a command that changes something succeeds, the window gets a `control-api-changed` event
(`{command}`) and reads the Library, the settings and each game's setup it had read (copy and
update) again, then draws the window and, when it's on, Big Picture again. Background work
started through the API (`setup_copy`, `install`) sends another one when it ends with its closing
event (`{command, job, state}`, state `succeeded`, `failed` or `cancelled`), since what it changes
is only in place then.

Settings: `patch_settings` (behind `POST /v1/settings` and `rlapi_update_settings`) reads,
merges and saves under the Library lock that every settings change in the launcher takes, so
patches made at the same time, and the launcher's own changes, are all kept. Every save of
`library.json` (the launcher's general save after an install, a setup and so on included) takes
its snapshot and writes it under that same lock, so saves reach the disk in the order they were
made: an older snapshot can't land over a newer patch, and two saves never share
`library.json.tmp`. `save_settings`
replaces every setting with the object it is given: a whole-object save built from settings read
before a patch (the window's Settings page, or a client calling `save_settings`) puts the old
values back. The window reading settings again after each API change narrows that gap but does
not close it; use `patch_settings` from clients.

Argument checks: a parameter is *missing* only when its key is absent. `null` is accepted where
the type takes it — a `serde_json::Value` (`plugin_store_set`'s `value`) or an `Option<T>`,
whose schemas are `{"type": [T, "null"]}` and never required (top level and inside structs,
for example `settings.lighter` and `settings.minimize_playing`). A `null` for a required
`String`, number or similar is refused as the wrong type.

## MCP

`initialize`, `ping`, `tools/list`, `tools/call` (protocol versions 2025-11-25, 2025-06-18,
2025-03-26, 2024-11-05). Tools:

- `rl_<command>` for each of the launcher's commands, with its parameters as the input schema
  (read-only ones carry `readOnlyHint`). Long ones answer with their job after 30 s.
  `saves_check` is not read-only: when the cloud has a newer save it puts it in place of the
  game's saves, so it has no `readOnlyHint`, and the window reads the state again after it.
  Nor are `check_updates` (it saves the version it works out for a game added without one) and
  `art_source` (it keeps the picture it's given in the covers folder).
- `rlapi_status`, `rlapi_list_commands`, `rlapi_describe_command`, `rlapi_invoke`,
  `rlapi_jobs`, `rlapi_job` (with `wait_ms`), `rlapi_cancel_job`, `rlapi_events`, `rlapi_app`,
  `rlapi_update_settings`, `rlapi_limitations`.

Results carry `structuredContent` (`{ok, result | error, job_id, job}`) and the same JSON as text;
command failures are `isError: true` results.

### stdio adapter

`tools/control-api/recomplauncher_mcp.py` (Python 3.8+, standard library only; also bundled in
the app as `RecompLauncher.app/Contents/Resources/control-api/recomplauncher_mcp.py`). It reads
the endpoint file on every message (so a restarted launcher is picked up), sends each JSON-RPC
line to `/mcp` and prints the answer; stdout carries only protocol messages, logs go to stderr.
It never uses an HTTP proxy. Options: `--tools meta` (only the `rlapi_*` tools; commands through
`rlapi_invoke`), `--autolaunch` (start the launcher, or turn its API on, when it isn't
reachable), `--data-dir` / `--identifier` / `--endpoint` for portable or test installs.

Claude Code:

```sh
claude mcp add recomplauncher -- python3 "$HOME/Applications/RecompLauncher.app/Contents/Resources/control-api/recomplauncher_mcp.py" mcp --autolaunch
```

Any MCP client (JSON config):

```json
{
  "mcpServers": {
    "recomplauncher": {
      "command": "python3",
      "args": ["/Users/you/Applications/RecompLauncher.app/Contents/Resources/control-api/recomplauncher_mcp.py", "mcp", "--autolaunch"]
    }
  }
}
```

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.recomplauncher]
command = "python3"
args = ["/Users/you/Applications/RecompLauncher.app/Contents/Resources/control-api/recomplauncher_mcp.py", "mcp", "--autolaunch"]
```

Clients that speak MCP over HTTP can use `mcp_url` from the endpoint file directly with the
`Authorization: Bearer` header (JSON responses; no SSE stream).

### Command line

```sh
A=tools/control-api/recomplauncher_mcp.py
python3 $A status
python3 $A commands --query rom
python3 $A describe setup_copy
python3 $A call adopt_pick '{"path": "/Users/you/Games/Super Mario 64/Ghostship.app"}'
python3 $A call install --arg id=zelda64recomp --async      # -> job id
python3 $A job job-7 --wait-ms 120000
python3 $A cancel job-7
python3 $A events --follow --names install,setup,game
python3 $A settings '{"rom_dirs": ["/Users/you/Games/ROMs"]}'
python3 $A endpoint      # where the file is and whether it answers (never the token)
```

Exit codes: 0 ok, 1 the call failed, 2 usage, 3 the launcher isn't reachable.

## What isn't available

- **Native dialogs.** File/folder pickers and ask/message boxes are opened by the page through
  the dialog plugin; the API doesn't open them. Every command they lead to takes the path or
  choice directly: `adopt_scan` / `adopt_pick {path}` / `adopt_add {items}` (installed games),
  `locate_game` / `set_exe` (a game's program), `scan_roms` / `rom_matches` /
  `setup_copy {id, path}` / `set_game_copy` (the player's game files), `set_update_copy`,
  `set_cover`, `plugin_inspect` / `plugin_install {path}`, `make_portable {target}`,
  `fs_places` / `fs_list` (browsing), and settings through `rlapi_update_settings`.
- **Window chrome** (drag, resize, maximize) is the core window plugin; use `rlapi_app`.
- **Flows the page strings together** (first-run setup, the setup wizard, the mod browser) are
  the same commands called in order.
- **Tauri plugin commands** (`plugin:*`) aren't exposed.
- Commands that only make sense with a person (browser sign-in: `account_start`/`account_poll`),
  that send data to recomplauncher.com (`usage_*`, `send_report`, `beta_feedback`...), or that
  open things on the desktop (`open_*`, `run_program`) are available and say so in their
  description; nothing calls them unless asked.

## Tests

```sh
cargo test --manifest-path src-tauri/Cargo.toml control      # transport, auth, dispatch, jobs, MCP, registry
python3 -m unittest discover -s tools/control-api -p 'test_*.py'
python3 tools/control-api/live_check.py --identifier <id> --report report.json [--writes --fixture-dir DIR]
```

The Rust tests run a real loopback server on Tauri's mock runtime and dispatch test commands
through `on_message` (State, AppHandle and window injection, errors, background jobs with
events and cancel, MCP), and check that the generated registry matches the handler list in
`lib.rs`. They also cover the review fixes: the environment override on the hand-off path,
concurrent settings patches (through the API and against the launcher's own writers), required
`null` versus absent, nullable `Option` schemas, and setup-run identity (direct cancel,
replacement, and a run that ends before its command answers). Round 2 adds: the launcher's
general save racing settings patches (`library.json` only moves forward and always parses, no
patch fails to save); `saves_check` not being read-only (no `readOnlyHint`); a cancel asked
for at once, while the command still waits for its lock, waiting and then stopping exactly that
run (never reported cancelled early, never finishing); run-bound cancels; and the real setup run
(`setup_work` / `setup_commit`) ending once, writing nothing when cancelled or replaced, with a
cancel or newer run that arrives while it writes waiting for it and finding it over. Round 3
adds: a panic while a setup run commits ending it with exactly one `error` (setup_commit
directly, and the whole run through setup_copy's thread with the Library lock poisoned);
`check_updates` and `art_source` not being read-only; and background jobs sending a change
notice when they end (done, cancelled), after the one sent as their command answered. The
end-to-end tests run on Unix only, matching the API. `live_check.py` checks a running launcher: auth and browser guards, every command's
dispatch, MCP over HTTP and stdio, jobs, events, and, on a test instance only, settings, ROM
scanning, import, play/stop and removal with a throwaway fixture.
