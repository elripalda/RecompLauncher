const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// Settings → RecompLauncher Updates: the launcher's own update checks can be turned off.
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const push = fs.readFileSync(path.join(__dirname, '../src-tauri/src/push.rs'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];
const helpers = app.match(/^const autoInstall = .*$/m)[0] + '\n' + app.match(/^const launcherUpdates = .*$/m)[0];

function settingsContext(settings) {
  const c = vm.createContext({
    S: {
      os: 'windows', settings, libs: [], library: {}, owned: {},
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab: 'updates', view: 'settings',
      launcherUpd: { version: '9.9' },
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['updates', 'Updates']], SET_ICONS: {}, DL_LIMITS: [0],
    okMark: () => '', portableRow: () => '', geRow: () => '', networkGroup: () => '', hoarderGroup: () => '',
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '',
  });
  c.window = c;
  c.clockRow = () => ''; c.monitorPicked = () => ''; c.monitorName = () => ''; c.setClock = () => {};
  c.applyEasySkip = () => {};
  vm.runInContext(helpers, c);
  return c;
}

test('RecompLauncher Updates shows as on for settings saved before the switch existed', () => {
  const c = settingsContext({});
  vm.runInContext(fn(app, 'pageSettings'), c);
  assert.match(c.pageSettings(), /aria-checked="true" aria-label="RecompLauncher Updates" data-act="toggle" data-k="launcher_updates"/);
});

test('RecompLauncher Updates shows as off once turned off', () => {
  const c = settingsContext({ launcher_updates: false });
  vm.runInContext(fn(app, 'pageSettings'), c);
  assert.match(c.pageSettings(), /aria-checked="false" aria-label="RecompLauncher Updates" data-act="toggle" data-k="launcher_updates"/);
});

test('Big Picture shows the same switch, on when unset', () => {
  const c = settingsContext({});
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="launcher_updates" data-sw="1"><span class="k">RecompLauncher Updates/);
  c.S.settings.launcher_updates = false;
  assert.match(c.pageSettings(), /data-toggle="launcher_updates" data-sw="0"><span class="k">RecompLauncher Updates/);
  assert.match(tv, /k === 'launcher_updates' \? S\.settings\[k\] !== false/);
});

test('turning it off saves it and drops the update banner; on again looks straight away', async () => {
  const c = settingsContext({});
  const saved = [];
  let looked = 0, sides = 0;
  c.invoke = async (command, args) => saved.push({ command, on: args.settings.launcher_updates });
  c.render = () => {};
  c.renderSide = () => { sides++; };
  c.lookForLauncher = () => { looked++; };
  c.fail = (error) => { throw error; };
  vm.runInContext('let settingsSaving = Promise.resolve();' + fn(app, 'saveSettings') + fn(app, 'act'), c);
  const toggle = { dataset: { act: 'toggle', k: 'launcher_updates' } };
  await c.act(toggle);
  assert.equal(c.S.launcherUpd, null);
  assert.equal(sides, 1);
  await c.act(toggle);
  assert.equal(looked, 1);
  assert.deepEqual(saved, [
    { command: 'save_settings', on: false },
    { command: 'save_settings', on: true },
  ]);
});

test('Check for Updates in the side menu still looks with the switch off', () => {
  assert.match(fn(app, 'checkLauncher'), /invoke\('launcher_update', \{ force: ask, manual: true \}\)/);
  assert.match(fn(app, 'lookForLauncher'), /invoke\('launcher_update', \{ force: false \}\)/);
  assert.match(lib, /if !launcher_looks\(&st\.lib\.lock\(\)\.unwrap\(\)\.settings, force \|\| manual\.unwrap_or\(false\)\)/);
});

test('the clock\'s look and a pushed build both respect the switch', () => {
  assert.match(lib, /wait_hours\(&h, \|s\| s\.launcher_check_hours\)\.await;\s*let st = h\.state::<App>\(\);\s*if !launcher_looks\(&st\.lib\.lock\(\)\.unwrap\(\)\.settings, false\) \{\s*continue;/);
  assert.match(push, /Push::Launcher => \{\s*let st = app\.state::<App>\(\);[^}]*if !crate::launcher_looks\(&st\.lib\.lock\(\)\.unwrap\(\)\.settings, false\) \{\s*return;/);
});

test('the fork build looks for itself in the fork\'s own releases, not recomplauncher.com', () => {
  assert.match(lib, /let url = fork_version_url\(\);/);
  assert.match(lib, /releases\/latest\/download\/version\.json/);
});

test('a newer build found by the clock is downloaded ahead, and Update Now installs that copy', () => {
  assert.match(lib, /if let Some\(v\) = newer_launcher\(&st, 0, false\)\.await \{\s*fetch_update\(&st, &v\)\.await;/);
  assert.match(lib, /let bytes = match ready_update\(&st\.dir, str_of\(&v, "version"\), &want\)/);
});

test('game updates are checked on their own clock, and the window looks when told', () => {
  assert.match(lib, /wait_hours\(&h, \|s\| s\.game_check_hours\)\.await;/);
  assert.match(app, /T\.event\.listen\('check-game-updates', \(\) => \{ if \(S\.settings\.check_updates !== false\) checkUpdates\(\); \}\);/);
});

test('each schedule shows only while its checks are on', () => {
  const c = vm.createContext({ S: { settings: {} }, t: (s) => s, esc: String });
  vm.runInContext(helpers + '\n' + app.match(/^const CHECK_EVERY = .*$/m)[0] + '\n' + ['hoardEveryLabel', 'checkEverySel', 'checkEveryRows'].map((n) => fn(app, n)).join('\n'), c);
  c.tf = (s, v) => s.replace('{n}', v.n);
  const row = (b, s, ctl) => `<row ${b}>${ctl}</row>`;
  c.sw = (k, on) => `<sw ${k}=${on}>`;
  const both = c.checkEveryRows(row, { check_updates: true }, c.sw);
  assert.match(both, /<row Check Games Every>[^]*data-k="game_check_hours"[^]*<option value="1" selected>Every hour/);
  assert.match(both, /<row Check RecompLauncher Every>[^]*data-k="launcher_check_hours"/);
  c.S.settings.launcher_updates = false;
  const games = c.checkEveryRows(row, { check_updates: true, game_check_hours: 6 }, c.sw);
  assert.doesNotMatch(games, /launcher_check_hours/);
  assert.match(games, /<option value="6" selected>Every 6 hours/);
  assert.equal(c.checkEveryRows(row, { check_updates: false }, c.sw), '');
});

test('update everything by itself is on unless turned off, for games and the launcher', () => {
  const c = vm.createContext({ S: { settings: {} }, t: (s) => s, esc: String, tf: (s, v) => s.replace('{n}', v.n) });
  vm.runInContext(helpers + '\n' + app.match(/^const CHECK_EVERY = .*$/m)[0] + '\n' + ['hoardEveryLabel', 'checkEverySel', 'checkEveryRows'].map((n) => fn(app, n)).join('\n'), c);
  const row = (b, s, ctl) => `<row ${b}>${ctl}</row>`;
  const sw = (k, on) => `<sw ${k}=${on}>`;
  const html = c.checkEveryRows(row, { check_updates: true }, sw);
  assert.match(html, /<sw auto_update_games=true>/);
  assert.match(html, /<sw launcher_auto_install=true>/);
  assert.match(c.checkEveryRows(row, { check_updates: true, auto_update_games: false, launcher_auto_install: false }, sw), /<sw auto_update_games=false>[^]*<sw launcher_auto_install=false>/);
  // Every found update starts, not only games set to Update automatically.
  assert.match(fn(app, 'checkUpdates'), /const allAuto = S\.settings\.auto_update_games !== false;/);
  assert.match(fn(app, 'checkUpdates'), /\(allAuto \|\| S\.library\[id\]\.auto_update\)/);
});

test('the launcher installs its downloaded update by itself only when out of sight and idle', () => {
  assert.match(lib, /fetch_update\(&st, &v\)\.await;\s*if auto_install_launcher\(&h, &v\)\.await \{\s*return;/);
  assert.match(lib, /let busy = !st\.running\.lock\(\)\.unwrap\(\)\.is_empty\(\) \|\| !st\.jobs\.lock\(\)\.unwrap\(\)\.is_empty\(\) \|\| hoard::status\(\)\.running;/);
  assert.match(lib, /let in_tray = std::env::args\(\)\.any\(\|a\| a == update_launch::TRAY_ARG\);/);
});
