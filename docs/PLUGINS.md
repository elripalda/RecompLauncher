# Making a RecompLauncher Plugin

A plugin adds a page to the launcher's sidebar, items to a game's right-click menu, or both. It's a small web page (HTML, CSS and JavaScript) packed in a `.rlplugin` file, which is a zip.

Players install it from **Settings → Plugins → Install From File…**, see what it asks for, and can turn it off or uninstall it any time.

There's a working example in [`docs/examples/plugin-example`](examples/plugin-example): Game Notes, which keeps a note for each game.

## What's in a plugin

```
my-plugin.rlplugin
├── plugin.json
├── index.html
├── app.js
├── style.css
└── icon.svg
```

`plugin.json` says what it is and what it needs:

```json
{
  "id": "example-notes",
  "name": "Game Notes",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "Keep notes for each game.",
  "icon": "icon.svg",
  "page": { "title": "Game Notes", "entry": "index.html", "icon": "icon.svg" },
  "menu": [{ "id": "note", "label": "Write a Note" }],
  "permissions": ["library.read", "network"],
  "sites": ["example.com"],
  "min_launcher": "0.90.0-beta.4"
}
```

| Field | |
|---|---|
| `id` | 3 to 40 lowercase letters, numbers and dashes. Installing a file with the same id updates the plugin. |
| `name`, `version` | Required. |
| `author`, `description`, `icon` | Shown in Settings and when the plugin is installed. |
| `page` | Its page in the sidebar. `entry` is the HTML file it opens. |
| `menu` | Up to 8 items for a game's right-click menu. Picking one opens the plugin's page and sends it a `menu` event. Needs a `page`. |
| `permissions` | What it may do (below). Keeping its own settings needs no permission. |
| `sites` | The sites it connects to: host names only, like `example.com`, or `*.example.com` for its subdomains too. Required with `network` or `install`. |
| `game_tab` | `{ "title": "Notes", "entry": "game.html" }`: a tab on every installed game's page showing that page, with `#game=<id>` on its address. Needs `library.read`. |
| `settings` | `true` adds a Settings button for it in Settings → Plugins. It opens your page and sends a `settings` event. Needs a `page`. |

## Permissions

| Permission | What the player sees | |
|---|---|---|
| `library.read` | See the games in your Library | Names, folders, play time. |
| `library.add` | Add folders to your Library | Asks the player each time. |
| `network` | Connect to the listed sites | Asks the player the first time for each site. They can allow it once or always. |
| `install` | Download and install add-ons from the listed sites | Mods, texture packs and other add-ons. Asks the player each time, naming the file, the site and where it goes. Shows in Downloads, where the player can cancel it. `.zip`, `.7z` and `.rar`, unpacked for the player. |
| `notify` | Show notifications in the launcher | |

## Talking to the launcher

Load the SDK first. The launcher serves it to every plugin page:

```html
<script src="/_sdk/rl.js"></script>
```

That gives the page an `rl` object. Every call returns a promise:

```js
await rl.library.list();                     // [{ id, name, version, folder, repo, played_secs, last_played }]
await rl.library.addFolder(folder, name);     // asks the player; { id, name }
await rl.library.files(gameId, 'mods');       // what's in a game's folder (or a folder in it): [{ name, folder, size, modified }]
await rl.net.fetch('https://example.com/api', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
                                              // { status, headers, body } (body as text; pass binary: true for base64)
await rl.install.zip('https://example.com/files/hd-textures.zip', 'HD Textures', { game: id, into: 'mods' });
                                              // asks the player, downloads, unpacks into that game's mods folder; { id, name }
await rl.install.zip(url, 'HD Textures', { game: id });
                                              // without into: straight into the game's folder, keeping the zip's folders
await rl.install.zip(url, 'HD Textures', { game: id, headers: { Cookie: 'session=...' } });
                                              // with headers, for a site the player signed in to through your page
await rl.install.zip(url, 'Toolkit');         // without game: unpacked into a folder of its own and added to the Library
                                              // (the player sees where and can pick another folder)
await rl.install.prefs();                     // { folder, ask }: where downloads go, and whether the player is asked first
await rl.install.chooseFolder();              // the player picks the folder (the launcher's own window)
await rl.install.setAsk(false);               // stop asking first (the launcher asks the player to confirm)
await rl.openUrl('https://example.com/page'); // in the player's browser; asks first for sites you don't list
await rl.storage.get('key');                  // anything JSON, up to 1 MB per plugin
await rl.storage.set('key', { any: 'value' }); // set to null to remove it
await rl.storage.all();
await rl.notify('Title', 'Text');             // needs notify
await rl.toast('Saved.');                     // only while the player is on your page
await rl.openGame(id);                        // a game's page (needs library.read)
await rl.info();                              // { launcher, os, language, theme }

rl.on('menu', ({ item, game }) => { /* game: { id, name, folder? } */ });
rl.on('game', ({ id, name, state, code, secs }) => { /* a game started ('running') or closed; needs library.read */ });
rl.on('settings', () => { /* the player pressed Settings for your plugin in Settings → Plugins */ });
rl.on('progress', ({ name, phase, done, total, speed }) => { /* your rl.install.zip downloads */ });
                                              // phase: prepare, download, verify, unpack, done, error, cancelled
```

When the player says no, or something fails, the promise rejects with an `Error` whose message says why.

## The rules a plugin page runs under

- It runs in a sandboxed frame with an origin of its own. It can't reach the launcher's page, its settings, the player's sign-in or their files.
- Requests and downloads go only to the sites you listed, redirects included. Headers you set (a sign-in cookie for your own site, say) go only to those sites, and the launcher's own sign-in never goes along.
- It can load its own files only. `fetch`, `XMLHttpRequest`, WebSockets, outside images and scripts are all blocked. Use `rl.net.fetch` for the sites you listed.
- No pop-up windows, `alert` or new tabs.
- The player can turn every plugin off at once in Settings → Plugins.

## Packing it

Zip the folder's contents (with `plugin.json` at the top) and rename the file to `.rlplugin`:

```
cd my-plugin
zip -r ../my-plugin.rlplugin .
```

On Windows, select the files, then right-click → Send to → Compressed (zipped) folder, and rename the `.zip` to `.rlplugin`.

A plugin can be up to 64 MB unpacked.

## Testing

Most of a plugin can be built and tried in a browser first, with a stand-in `rl` object. Then install it from Settings → Plugins. Install the file again after a change: it updates in place and keeps its settings.
