# What the launcher asks recomplauncher.com

Accounts, cloud saves, sending games to a PC, betas, reports and usage, and the launcher's own
updates. The launcher side is `src-tauri/src/cloud.rs`, `saves.rs` and `usage.rs`.

## Signing a PC in

1. `POST /api/launcher.php?a=link-start` with `{"device", "os", "version"}`
   → `{"code": "ABCD-EFGH", "poll", "url", "interval", "expires"}`. The code lasts 10 minutes.
2. The launcher opens `url` (recomplauncher.com/activate?code=...), or shows the code and a QR code
   in Big Picture mode. Not signed in on the website: sign-in on recompstore.com, then straight back.
3. The player presses Sign in on that page.
4. The launcher asks every few seconds: `POST ?a=link-poll` with `{"poll"}`
   → `pending`, `denied`, `expired`, or `ok` with `token` and `account` (uid, username, avatar).
   The token is handed over once; the website only keeps its hash.

Every other call sends the token as `X-RL-Token` (and `Authorization: Bearer`). A 401 means the PC
was signed out (from the account page, or the account is gone): the launcher forgets the token.

## Cloud saves

| Call | |
| --- | --- |
| `GET ?a=me` | The account, and `saves: {used, quota}` |
| `GET ?a=saves&game=ID` | That game's copies, newest first: `v`, `hash`, `size`, `made`, `device`, `played` |
| `PUT ?a=save-put&game=&hash=&made=&played=&device=` | The body is the copy (a zip with `rlsave.json`) |
| `GET ?a=save-get&game=&v=` | The copy |
| `POST ?a=save-delete` | `{"game", "v"}` |
| `POST ?a=signout` | This PC's token stops working |

The website keeps the newest five copies per game and 150 MB per account, refuses game files,
programs and archives inside a copy, and doesn't keep a copy whose `hash` matches the newest one.

`hash` is the checksum of the whole set of save files (see `saves::set_hash`), so two PCs with the
same saves have the same hash. The launcher remembers the hash it last agreed on with the cloud:

- Cloud newest = this PC's saves: play.
- Only the cloud changed since: the cloud copy is put in place, then play.
- Only this PC changed: play; it goes up afterwards.
- Both changed: the player picks. The other one is always kept as a copy.

## The account page

With the website's sign-in cookie: `GET ?a=pcs`, `POST ?a=pc-remove {id}`, `GET ?a=cloud`,
`GET ?a=cloud-get&game=&v=`, `POST ?a=cloud-delete {game, v}`. Deleting the account deletes its
PCs and cloud saves too.

## Get it on my PC

The website's game page has a button that sends a recomp to one of your signed-in PCs. It only
lists PCs whose system the recomp has a build for.

- Website (cookie): `GET ?a=send-pcs&game=ID` → the PCs that can run it; `POST ?a=send {game, device}`.
- Launcher (token): `GET ?a=inbox` every 90 seconds → `items: [{id, game, name, sent}]`. The
  launcher installs each one (with a notification when its window is hidden), then
  `POST ?a=inbox-done {id, result}` with `installing`, `installed` or `no`. Sends older than 30
  days are dropped.

## Betas

A beta is a recomp's test builds from its own GitHub releases: the newest pre-release, or one
release by tag. Access is `open` (anyone signed in joins), `request` (the admin lets them in) or
`invite` (only people the admin adds).

- Launcher (token): `GET ?a=betas` → `betas: [{id, app, name, title, notes, channel, tag, access,
  status, systems}]`, where `status` is `in`, `asked` or empty. `POST ?a=beta-join {id}` →
  `{status, betas}`; `POST ?a=beta-leave {id}`.
- Website (cookie): `GET ?a=my-betas`, `POST ?a=web-beta-join {id}`, `POST ?a=web-beta-leave {id}`.

Playing the beta installs that release (it stays on it, so updates don't move it). Back to the
release installs the newest normal one.

## Reports and usage

No token needed; each launcher has a random 32-character install ID (`install.id` in its data
folder). Signed in, the token goes along and the account is kept with it. These work even when
accounts are turned off on the website, and are rate limited per IP.

- `POST ?a=report {install, app, verdict, sys, version, note, mins, gpu, cpu, ram_gb, osver}`:
  "How did it run?" after a session of 10 minutes or more, once per game and version. Shows with the
  game's other reports (`src = launcher`), with the PC's parts.
- `POST ?a=hello {install, version, os, osver, cpu, cores, ram_gb, gpu, gpu_driver, screen, lang,
  pads, games, bigpicture}`: once a day.
- `POST ?a=events {install, events: [{kind, at, game, version, secs, code, detail}]}`: `session`
  (30 s or more), `crash` (closed in under 15 s with an error code), `install`, `update`, `error`,
  `uninstall`. Kept on the PC in `cache/events.json` until they get through (200 at most).

Hello and events only go when "Share usage and diagnostics" is on (Settings, General; on by default,
and in first-time setup).

## RecompLauncher's own updates

`GET /launcher/version.json` → `{version, notes, url, sha256, size, date}`. The launcher checks on
start and every 6 hours, downloads `url`, checks `sha256`, and runs the installer with `/P /R`
(quiet, then opens again). `notes` (Markdown) shows once as What's new.
