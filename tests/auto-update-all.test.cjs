const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];
const helpersSrc = [/^const autoInstall = .*$/m, /^const autoUpdateAll = .*$/m, /^const autoUpSkipped = .*$/m]
  .map((re) => app.match(re)[0]).join('\n') + '\n' + fn(app, 'autoUpdates') + '\n' + fn(app, 'toggleAutoUpdate');

function settingsContext(settings) {
  const c = vm.createContext({
    S: {
      os: 'windows', settings, libs: [], library: {}, owned: {},
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab: 'updates', view: 'settings',
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['updates', 'Updates']], SET_ICONS: {}, DL_LIMITS: [0],
    okMark: () => '', portableRow: () => '', geRow: () => '', networkGroup: () => '',
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, ownedGames: () => [], bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '', setClock: () => {},
  });
  c.window = c;
  c.clockRow = () => ''; c.monitorPicked = () => ''; c.monitorName = () => '';
  vm.runInContext(helpersSrc, c);
  return c;
}

// Arrays made inside the sandbox aren't this file's arrays, so they're compared as plain data.
const plain = (x) => JSON.parse(JSON.stringify(x));

// A library of the given games, and a record of what a game's own switch saves.
function switchContext(settings, library) {
  const c = settingsContext(settings);
  c.S.library = library;
  c.calls = [];
  c.invoke = async (command, args) => { c.calls.push({ command, autoUpdate: args.autoUpdate }); return { ...library[args.id], auto_update: args.autoUpdate }; };
  c.saveUi = async (patch) => { c.calls.push({ ui: patch }); c.S.settings.ui = { ...(c.S.settings.ui || {}), ...patch }; };
  c.toast = () => {}; c.render = () => {};
  c.fail = (error) => { throw error; };
  return c;
}

test('Update All Games Automatically is off for settings saved before the switch existed', () => {
  const c = settingsContext({});
  vm.runInContext(fn(app, 'pageSettings'), c);
  assert.match(c.pageSettings(), /aria-checked="false" aria-label="Update All Games Automatically" data-act="toggle" data-k="auto_update_all"/);
});

test('Update All Games Automatically shows as on once the player turns it on', () => {
  const c = settingsContext({ auto_update_all: true });
  vm.runInContext(fn(app, 'pageSettings'), c);
  assert.match(c.pageSettings(), /aria-checked="true" aria-label="Update All Games Automatically" data-act="toggle" data-k="auto_update_all"/);
});

test('Big Picture shows the same Update All Games Automatically switch', () => {
  const c = settingsContext({});
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="auto_update_all" data-sw="0"><span class="k">Update All Games Automatically/);
  c.S.settings.auto_update_all = true;
  assert.match(c.pageSettings(), /data-toggle="auto_update_all" data-sw="1"><span class="k">Update All Games Automatically/);
});

test('a game updates by itself with only its own switch on', () => {
  const c = switchContext({}, { a: { auto_update: true }, b: { auto_update: false } });
  assert.equal(c.autoUpdates('a'), true);
  assert.equal(c.autoUpdates('b'), false);
});

test('every game updates by itself with the Settings switch on', () => {
  const c = switchContext({ auto_update_all: true }, { b: { auto_update: false } });
  assert.equal(c.autoUpdates('b'), true);
});

test('a game turned off in its Manage tab waits, even with the Settings switch on', () => {
  const c = switchContext({ auto_update_all: true, ui: { auto_update_skip: ['b'] } }, { b: { auto_update: false } });
  assert.equal(c.autoUpdates('b'), false);
});

test('a local build never updates by itself, even with the Settings switch on', () => {
  const c = switchContext({ auto_update_all: true }, { l: { local: true, auto_update: true } });
  assert.equal(c.autoUpdates('l'), false);
});

test('with the Settings switch on, a game\'s own switch goes on the skip list and off it again', async () => {
  const c = switchContext({ auto_update_all: true }, { b: { name: 'B', auto_update: false } });
  await c.toggleAutoUpdate('b');
  assert.equal(c.autoUpdates('b'), false);
  await c.toggleAutoUpdate('b');
  assert.equal(c.autoUpdates('b'), true);
  assert.deepEqual(plain(c.calls), [{ ui: { auto_update_skip: ['b'] } }, { ui: { auto_update_skip: [] } }]);
});

test('with the Settings switch on, turning off a game that had its own switch on clears that too', async () => {
  const c = switchContext({ auto_update_all: true }, { a: { name: 'A', auto_update: true } });
  await c.toggleAutoUpdate('a');
  assert.equal(c.autoUpdates('a'), false);
  assert.deepEqual(plain(c.calls), [{ command: 'set_game_options', autoUpdate: false }, { ui: { auto_update_skip: ['a'] } }]);
});

test('with the Settings switch off, a game\'s own switch saves only the game', async () => {
  const c = switchContext({}, { b: { name: 'B', auto_update: false } });
  await c.toggleAutoUpdate('b');
  assert.equal(c.autoUpdates('b'), true);
  assert.deepEqual(plain(c.calls), [{ command: 'set_game_options', autoUpdate: true }]);
});

test('Big Picture\'s game options carry the same switch', () => {
  assert.match(tv, /window\.autoUpdates\(id\); return \{ label: on \? t\('Turn off automatic updates'\) : t\('Update automatically'\)/);
  assert.match(tv, /run: async \(\) => \{ await window\.toggleAutoUpdate\(id\); draw\(true\); \}/);
});

test('the update check starts the updates of games that update by themselves', () => {
  assert.match(fn(app, 'checkUpdates'), /\.filter\(\(id\) => autoUpdates\(id\) && !S\.running\.has\(id\)/);
});

test('turning the Settings switch on starts the updates already waiting', async () => {
  const c = settingsContext({});
  const saved = [];
  let checks = 0;
  c.invoke = async (command, args) => saved.push(args.settings.auto_update_all);
  c.render = () => {};
  c.fail = (error) => { throw error; };
  c.checkUpdates = () => { checks += 1; };
  vm.runInContext('let settingsSaving = Promise.resolve();' + fn(app, 'applyEasySkip') + fn(app, 'saveSettings') + fn(app, 'act'), c);
  const toggle = { dataset: { act: 'toggle', k: 'auto_update_all' } };
  await c.act(toggle);
  await c.act(toggle);
  assert.deepEqual(saved, [true, false]);
  assert.equal(checks, 1);
});

test('Big Picture\'s Turn off automatic updates reads in Title Case in English', () => {
  const src = fs.readFileSync(path.join(__dirname, '../ui/titlecase.js'), 'utf8');
  const c = vm.createContext({
    document: { body: null, documentElement: { lang: 'en' }, addEventListener: () => {} },
    MutationObserver: class { observe() {} },
  });
  c.window = c;
  vm.runInContext(src, c);
  assert.equal(c.titleCase('Turn off automatic updates'), 'Turn Off Automatic Updates');
});
