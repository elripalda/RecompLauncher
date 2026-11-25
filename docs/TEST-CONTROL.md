# Test control

Test control lets an AI agent (or a script) use RecompLauncher the way a tester would, so a new
build can be checked before it ships: look at the window, press buttons, open pages, start
installs, setups and games, press controller buttons, and read the logs. It's an
[MCP](https://modelcontextprotocol.io) server built into the launcher.

It's off unless you turn it on yourself when you start the launcher. Players never see it.

## Turning it on

1. Close RecompLauncher, including from the icon by the clock. A second copy only hands over to
   the one already running.
2. Pick a password-like token, 16 characters or more, with no spaces.
3. Start the launcher with `RECOMPLAUNCHER_TEST_CONTROL` set to that token.

Windows (PowerShell):

```powershell
$env:RECOMPLAUNCHER_TEST_CONTROL = [guid]::NewGuid().ToString('N')
$env:RECOMPLAUNCHER_TEST_CONTROL   # note it down
& "$env:LOCALAPPDATA\RecompLauncher\recomplauncher.exe"
```

Linux:

```sh
export RECOMPLAUNCHER_TEST_CONTROL=$(openssl rand -hex 16)
echo $RECOMPLAUNCHER_TEST_CONTROL
recomplauncher   # or ./RecompLauncher.AppImage
```

The window shows a small yellow **Test control on** mark in the corner, and the launcher's trail
(`launcher-trail.log` in its data folder) says `Test control: on, MCP at http://127.0.0.1:47811/mcp`.
If the token is too short or the port is taken, the trail says why and the launcher starts as
usual, without test control.

`RECOMPLAUNCHER_TEST_CONTROL_PORT` picks another port (1024 to 65535).

## Connecting an agent

The server is at `http://127.0.0.1:47811/mcp` (MCP over HTTP), with the token sent as
`Authorization: Bearer <token>`. In Claude Code:

```sh
claude mcp add --transport http recomplauncher http://127.0.0.1:47811/mcp --header "Authorization: Bearer <token>"
```

Other MCP clients take the same URL and header. The agent has to run on the same PC (or VM) as
the launcher: the server only answers on 127.0.0.1.

## What it can do

| Tool | What it does |
| --- | --- |
| `launcher_status` | Version, OS, data folder, the window, installed games, games running, downloads and setups in progress |
| `ui_snapshot` | What's in front (a dialog, Big Picture mode or the page): headings, messages, and everything that can be pressed, each with a `ref` |
| `ui_click` | Clicks an element, named by `ref`, a CSS `selector`, or the `text` on it |
| `ui_type` | Types into a box, optionally pressing Enter after |
| `ui_key` | Presses a key (Enter, Escape, arrows, letters, with modifiers) |
| `ui_navigate` | Opens a page: home, discover, library, achievements, game (with `id` and `tab`), downloads, settings (with `tab`), shots, soon, saves, back, bigpicture, or desktop (leaves Big Picture) |
| `ui_wait_for` | Waits for text, an element or a page to show (or to go) |
| `controller_press` | Presses a controller button, as a real controller would (Big Picture mode and the desktop highlight) |
| `launcher_command` | Calls one of the launcher's own commands the way the window does (install, setup, play...) |
| `ui_eval` | Runs JavaScript in the window, for checks the other tools can't make |
| `read_logs` | The launcher's trail, a game's output from its last start, or the window's console errors |
| `recent_events` | What the launcher told the window: download progress, setups, games starting and closing, messages |
| `screenshot` | A picture of the window, or of the whole screen (Windows only) |
| `launcher_window` | Shows and focuses the window, or minimizes it |

A good loop for an agent: `ui_snapshot`, act with `ui_click` or `controller_press`, then
`ui_wait_for` or a new `ui_snapshot` to see what changed. Refs last until the page is drawn again.

## Keep in mind

- It drives a real launcher on a real PC: installs, uninstalls, settings and cloud saves really
  change. Use a test PC or VM, and an account made for testing.
- Anyone who has the token and can reach 127.0.0.1 on that PC can control the launcher while it
  runs. Web pages are refused, even with the token. Don't reuse a token you use anywhere else.
- Turning it off: close the launcher and start it again without the variable.

## How it's put together

`src-tauri/src/test_control/`: `mod.rs` reads the variables and runs the server, `http.rs` reads
requests and turns away anything without the token or from a web page, `mcp.rs` has the tools,
`page.rs` sends steps to the window, and `bridge.js` is the window's side, put in the page only
when test control is on. The HTTP and MCP handling started from the local control API in
[#10](https://github.com/elripalda/RecompLauncher/pull/10) by Alex Southwell.
