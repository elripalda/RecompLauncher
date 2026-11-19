const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];
const helpersSrc = [/^const autoInstall = .*$/m, /^const autoUpdateAll = .*$/m, /^const autoUpdates = .*$/m]
  .map((re) => app.match(re)[0]).join('\n');

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
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '', setClock: () => {},
  });
  c.window = c;
  c.clockRow = () => ''; c.monitorPicked = () => ''; c.monitorName = () => '';
  vm.runInContext(helpersSrc, c);
  // A const stays inside the script it was declared in, so the tests ask for it by name.
  c.autoUpdates = vm.runInContext('autoUpdates', c);
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
  const c = settingsContext({});
  assert.equal(c.autoUpdates({ auto_update: true }), true);
  assert.equal(c.autoUpdates({ auto_update: false }), false);
});

test('every game updates by itself with the Settings switch on', () => {
  const c = settingsContext({ auto_update_all: true });
  assert.equal(c.autoUpdates({ auto_update: false }), true);
});

test('a local build never updates by itself, even with the Settings switch on', () => {
  const c = settingsContext({ auto_update_all: true });
  assert.equal(c.autoUpdates({ local: true, auto_update: true }), false);
});

test('the update check starts the updates of games that update by themselves', () => {
  assert.match(fn(app, 'checkUpdates'), /\.filter\(\(id\) => autoUpdates\(S\.library\[id\]\) && !S\.running\.has\(id\)/);
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
