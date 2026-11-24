# What the launcher asks recomplauncher.com

Accounts, cloud saves, sending games to a PC, betas, reports and usage, and the launcher's own
updates. The website side is `api/launcher.php`, `activate.php` and `admin/inc/rl_cloud.php`; the
launcher side is `src-tauri/src/cloud.rs`, `saves.rs` and `usage.rs`.

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

The website keeps the newest five copies per game and 300 MB per account, refuses game files,
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

## Playtime

Signed in, each PC sends its own running total per game (seconds played on that PC, ever), and the
website adds the PCs up. Because a PC always sends its total and never "since last time", a resend
or a lost request never counts anything twice.

| Call | |
| --- | --- |
| `POST ?a=play` | `{"pc", "games": [{"game", "secs", "last", "count", "longest", "recent"}]}`, up to 500 and 1 MB → `{"ok": true}`. The website keeps the larger of what it has and what's sent, so nothing goes down. Bad entries (`secs` over 20 years, `last` more than a day ahead, a malformed id) are skipped, not the whole request. |
| `GET ?a=play` | `{"games": {ID: {"secs", "last", "count", "longest", "recent"}}, "titles": {TITLE: {"secs", "last"}}}`, summed over the account's PCs |
| `GET ?a=spons` | `{"s": [{"id", "slot", "img", "alt", "house"}]}`: sponsor spots for this account (Home's `app_home`, between Ready to Play and Recently Updated; `cloud::spons_mine`, `sponsSync` in app.js). Views go to `/api/spons.php?seen=<id>` (`cloud::spons_seen`) and a click opens `/api/spons.php?go=<id>`. Empty when there's nothing for this account. Used only while the Home list below hasn't come. |

`count`, `longest` and `recent` draw a game's "Your play time" card (this week, this month,
sessions, longest, the last two weeks) over all the PCs:

- `count` is how many sessions that PC kept of the game (10 seconds or more each, its newest 500)
  and `longest` the longest of them in seconds. Like `secs`, the website keeps the larger of what
  it has and what's sent per PC; `GET` gives `count` summed over the PCs and `longest` the largest.
- `recent` is that PC's sessions of the last 31 days, as `[start, secs]` (start in Unix seconds,
  both whole numbers above zero), oldest first. The website adds them to what it had for that PC
  (a session is the same one when both numbers match, so a resend adds nothing), drops sessions
  older than 31 days, and skips a bad one (start more than a day ahead, longer than 20 years) on
  its own. `GET` gives every PC's recent sessions together, oldest first.
- A site without them leaves the card to the PC's own sessions. Titles don't carry them.

`pc` is the PC's play ID: 32 lowercase hex characters, random, kept in `play.id` in the
launcher's data folder (`cloud::play_id`). It's the same each time that PC signs in, which the
sign-in token isn't, so the website keeps play rows by account and play ID rather than by sign-in:
signing out and back in then adds to the same rows instead of counting that PC's whole time a
second time. When a send has a good `pc`, the rows that sign-in sent before under itself move to
that play ID, merged as above (the larger of each number, recent sessions joined). A send without
one (an older launcher) is kept by its sign-in as before. It's used for nothing but play time, and
isn't the usage ID (`install.id`), which only goes when the player shares usage and Settings can
reset. Rows from a sign-in that's already gone stay as they are.

- `game` is the recomp's id, or `recomp--game` for one game of a recomp that carries several.
  A developer's own builds (`local-…`, `own-…`) aren't sent.
- A title is the sum of its recomps in the catalog's `titles[].pc`, and a replaced listing counts
  as its new id. `recomp--game` rows don't count toward titles until the catalog maps those games.
- The launcher sends everything after signing in and then once a day (installed games and what
  it kept from uninstalled ones), one game after each session, and reads the totals with the
  account and every 10 minutes (`cloud::playtime_push`, `playtime_pull`; `playSync` in app.js).
- 120 sends and 120 reads an hour per PC. Any failure, a 401 included, just leaves this PC's own
  numbers showing: these calls never sign the PC out, and they rest on their own in net.rs.
- `save-put`'s `played` only labels save copies (the "Which save do you want?" picker) and isn't
  counted.
- `tools/fake-play-site.cjs` stands in for the website to try this locally (`RL_API`).

Account page (cookie): `GET ?a=playtime` (each game's total and each PC's share), `POST
?a=playtime-clear {game?}` (one game, or everything). Removing a PC keeps its rows; deleting the
account deletes them. Each play ID's share is one PC, named after the sign-in that last sent it.

## Get it on my PC

The website's game page has a button that sends a recomp to one of your signed-in PCs. It only
lists PCs whose system the recomp has a build for.

- Website (cookie): `GET ?a=send-pcs&game=ID` → the PCs that can run it; `POST ?a=send {game, device}`.
- Launcher (token): `GET ?a=inbox` when it opens, then about every 15 minutes → `items: [{id, game, name, sent}]`. The
  launcher installs each one (with a notification when its window is hidden), then
  `POST ?a=inbox-done {id, result}` with `installing`, `installed` or `no`. Sends older than 30
  days are dropped.

## Betas

A beta is a recomp's test builds from its own GitHub releases: the newest pre-release, or one
release by tag. Access is `open` (anyone signed in joins), `request` (the admin lets them in) or
`invite` (only people the admin adds). The admin page is `admin/launcher.php`, Betas tab.

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
  `uninstall`. Kept on the PC in `cache/events.json` and sent together 10 to 15 minutes after the
  first one (or with the next hello), 200 at most.
- `POST ?a=recomp-request {a, install, url, note, version}`: Request a Recomp, from the Coming Soon
  page. `url` is a project link the launcher has already tidied (https, no sign-in, no trailing
  slash or `.git`, 300 characters at most) on GitHub or GitLab only: the player picks one and types
  `developer/project` (GitLab can have groups in between). `note` is up to 500 characters, often empty. Answers `{ok: true}`, or
  `{ok: false, error}` with words the launcher shows as they are (already requested, too many
  requests today). Until the site has it, the launcher says the request didn't go through.

Hello and events only go when "Share usage and diagnostics" is on (Settings, General; on by default,
and in first-time setup). The admin page shows them on its Players and Problems tabs.

## RecompLauncher's own updates

`GET /launcher/version.json` → `{version, notes, url, sha256, size, date}`. The launcher checks on
start (reusing an answer from the last hour) and about every 6 hours, downloads `url`, checks `sha256`, and runs the installer with `/P /R`
(quiet, then opens again). `notes` (Markdown) shows once as What's new. The admin page's Updates
tab uploads an installer to `/launcher/` and publishes version.json.

For Linux, publish a `linux` object with an HTTPS AppImage `url` (from GitHub, this site, or the
update server itself, which `RL_API` can point at a local copy for testing), its own 64-digit `sha256`,
and optional `version`, `notes`, `min` and `size`. Only `version` and `notes` fall back to the
shared fields; Windows checksums and minimum versions never apply to Linux. The AppImage
updater requires a newer version and a valid checksum, bounds the download to 512 MiB, and
checks the type-2 AppImage container and native runtime architecture before staging it.
Missing or invalid AppImage metadata produces an error inside the launcher, allowing retry.

The updater stages in a private directory beside the running AppImage, so its folder must be
writable. That's checked before the download: when this account can't change the folder, a
window says so and offers the new AppImage to download instead. A system-shell helper waits
for the old process to exit, atomically replaces the same filename, and starts it in the
current desktop or Big Picture mode if the window is still open. Quitting during preparation
cancels the update; closing to the tray leaves it closed after replacement. Symlinks and
shortcuts keep their original paths. The previous image is restored if the new runtime exits
with an error in the first two seconds. This checks immediate launch failure; it does not
verify that the new UI rendered or recover from later crashes. Preparation failures leave the
installed image untouched. If the swap fails, the unchanged launcher opens again.

A failed update is reported the way it is on Windows and Mac: `update-pending.json` is written
before the handoff, and when the old version is the one that starts next, it shows "The Update
Didn't Start" with Try Again. The second failure from the same file (same version, `url` and
`sha256`) stops it being offered until version.json changes it. The helper notes what went
wrong in `launcher-trail.log`. A staging directory an interrupted update left behind is
removed at the next start once it's an hour old.

The `.deb` and plain Linux executable keep the download-page fallback. Flatpak updates remain
managed by Flatpak. On every platform, an update error now closes the progress dialog so the
player can retry (on Windows it used to stay open); otherwise Windows installs as before.

## How often the launcher asks

Everything the launcher asks this server goes to one address, `API_BASE` in `src-tauri/src/net.rs`.

- `pc-catalog.json`: kept on disk. Asked for when the launcher opens if the copy is more than 6
  hours old (in the background; the window shows the copy on disk straight away), when the player
  presses Refresh, or after a `catalog` push.
- `api/gh.php?repo=`: kept on disk for 6 hours per repo. Asked for when the player opens a game's
  page, installs or updates, and on start for installed games whose copy is older than 6 hours.
- `api/pc.php?a=counts` and `a=feedback`: only when a page that shows them opens, kept for 1 hour.
- `launcher/version.json`: on start (an answer from the last hour is reused), then about every
  6 hours, or after a `launcher` push.
- `api/launcher-ping.php`: on open, then every 30 minutes at the most often. A `next` (seconds) in
  the answer can make it less often, never more.
- `api/launcher.php`: the inbox about every 15 minutes while signed in; events in batches; cloud
  saves before and after play.
- Pushes: each launcher waits a random time of up to 15 minutes before acting on one, and acts on
  each kind at most every 30 minutes (the list, every hour).

When the server doesn't answer, times out, or answers 403, 408, 429 or 5xx, the launcher stops
asking it for 30 seconds, then 1, 2, 4 minutes and so on up to an hour, each with a random part, and
works from its copies on disk meanwhile (the window shows "Offline"). `Retry-After` on a 429 or 503
is respected. Requests have short timeouts (10 seconds, 20 for the list).

## Project maintenance status

Catalog app entries may supply `archived` (a boolean), `status` (`ended`, `abandoned`,
`paused`, or `complete`), and an optional `status_note`. `abandoned` uses the same
existing warning as `ended`. `complete` takes priority over an archived flag; an old
release date alone means quiet, not abandoned. These are catalog fields, not setup
manifest properties in `recomplauncher.schema.json`.

When release lookup falls back to GitHub, the launcher also asks
`GET /repos/{owner}/{repo}` for its `archived` flag. The request is bounded and runs
alongside release lookup; a failed or rate-limited metadata response does not reject
otherwise available releases. The last known flag is cached separately and included
as optional `archived` in the launcher's release response, without changing its `data`
array. Cached status is exposed as `github_archived` in catalog and player-added entries
that lack an explicit flag. Fresh release metadata supersedes that cached fallback;
an explicit catalog `archived` boolean stays authoritative. Desktop and Big Picture use the
same status and keep installation available.

This fallback does not infer abandonment from repository activity and does not change
how the separately hosted catalog service collects its metadata.

## Home's order (`/api/home.php`)

`GET /api/home.php?page=app` (signed out) or `?page=app&me=1` with the account's token (adds the blocks for App Testers): what goes on Home, in order, set on the website's admin (Launcher > Home page). `cloud::home_layout` asks for it at start, every 10 minutes, and when Home opens (at most once a minute), and keeps the last good answer in `cache/home.json` for offline use.

`{"v": 1, "items": [...], "hide": [row ids]}`. Items: `{"k": "row", "id"}` (one of Home's own rows: `played`, `favs`, `ready`, `updated`, `follow`, `found`, `trend`, `new`, `easy`, `news`), `{"k": "box", "id", "title", "text", "icon", "btn", "act", "arg", "when", "close"}` (an info box; `act` is `link`, `game`, `rom-add` or `go:discover|soon|library|settings`; `when` is ``, `no_folders`, `signed_out` or `signed_in`), `{"k": "banner", "id", "slot", "sp": {"id", "img", "alt", "house"}}` (counted like the sponsor spots above).

The launcher (`homePlan` in app.js) skips kinds and rows it doesn't know, puts back a row the list doesn't name after the row it usually follows, and keeps its built-in order (with the `a=spons` banner) when there's no list or its `v` isn't 1. A closed box is remembered in settings (`ui.home_closed`).

Big Picture's Home (`tvHomeRows` in tv.js) follows the same list under its row of your games. It has its own rows, matched by the same ids (`played` is Continue playing, shown when that row holds your pinned games; `found`, `ready`, `easy`, `news`, `updated`, `new`, `trend`); `favs` and `follow` aren't in Big Picture, so their place is skipped. Info boxes and banners are as wide as a row and picked with the controller (a box's button does what it does on the desktop, `homeBoxAct`); a box with nothing to press shows with the row under it. Without the list it keeps its own order, with no banner.

