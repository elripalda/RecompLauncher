const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];
const autoInstallSrc = app.match(/^const autoInstall = .*$/m)[0];
// The part of the download-finished handler that decides what opens next.
const doneHandler = app.slice(app.indexOf("T.event.listen('install'"), app.indexOf("T.event.listen('game'"));

function settingsContext(settings, setTab = 'updates') {
  const c = vm.createContext({
    S: {
      os: 'windows', settings, libs: [], library: {}, owned: {},
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab, view: 'settings',
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['updates', 'Updates']], SET_ICONS: {}, DL_LIMITS: [0],
    okMark: () => '', portableRow: () => '', geRow: () => '', networkGroup: () => '',
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '',
  });
  vm.runInContext(autoInstallSrc, c);
  return c;
}

test('Auto-Install shows as on for settings saved before the switch existed', () => {
  const c = settingsContext({});
  vm.runInContext(fn(app, 'pageSettings'), c);
  assert.match(c.pageSettings(), /aria-checked="true" aria-label="Auto-Install" data-act="toggle" data-k="auto_install"/);
});

test('Auto-Install shows as off once the player turns it off', () => {
  const c = settingsContext({ auto_install: false });
  vm.runInContext(fn(app, 'pageSettings'), c);
  assert.match(c.pageSettings(), /aria-checked="false" aria-label="Auto-Install" data-act="toggle" data-k="auto_install"/);
});

test('Big Picture shows the same Auto-Install switch', () => {
  const c = settingsContext({});
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="auto_install" data-sw="1"><span class="k">Auto-Install/);
  c.S.settings.auto_install = false;
  assert.match(c.pageSettings(), /data-toggle="auto_install" data-sw="0"><span class="k">Auto-Install/);
});

test('the desktop switch turns an unset Auto-Install off, then on again', async () => {
  const c = settingsContext({});
  const saved = [];
  c.invoke = async (command, args) => saved.push({ command, on: args.settings.auto_install });
  c.render = () => {};
  c.fail = (error) => { throw error; };
  vm.runInContext('let settingsSaving = Promise.resolve();' + fn(app, 'saveSettings') + fn(app, 'act'), c);
  const toggle = { dataset: { act: 'toggle', k: 'auto_install' } };
  await c.act(toggle);
  await c.act(toggle);
  assert.deepEqual(saved, [
    { command: 'save_settings', on: false },
    { command: 'save_settings', on: true },
  ]);
});

test('Big Picture treats an unset Auto-Install as on', () => {
  assert.match(tv, /k === 'auto_install' \? S\.settings\[k\] !== false/);
});

test('a finished installer download opens its installer only with Auto-Install on', () => {
  assert.match(doneHandler, /const auto = autoInstall\(\);/);
  assert.match(doneHandler, /if \(isInstallerApp\(byId\(id\)\)\) \{ if \(auto && !busy/);
});

test('a finished Easy Install download opens its setup only with Auto-Install on', () => {
  assert.match(doneHandler, /su\.needs && !su\.copy && !auto\) toast\(tf\('\{name\} is installed\. Set it up from its page\.'/);
});

test('Easy Install skips Pick Your Copy First with Auto-Install off', () => {
  const start = fn(app, 'startInstall');
  assert.match(start, /if \(autoInstall\(\) && !S\.library\[id\] && !tag && !anyway && turnkey\(byId\(id\)\)/);
});
