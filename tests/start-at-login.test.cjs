const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];

function settingsContext(os, can, on) {
  return vm.createContext({
    S: {
      os, canStartAtLogin: can, settings: { start_at_login: on }, libs: [], library: {}, owned: {},
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab: 'general', view: 'settings',
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['general', 'General']], SET_ICONS: {},
    okMark: () => '', portableRow: () => '', geRow: () => '', igOn: () => false,
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, ownedGames: () => [], bgMode: () => '', langName: () => 'English', clockSelect: () => '', clockRow: () => '', hideNameItem: () => '', setClock: () => {}, window: { monitorPicked: () => null, monitorName: () => '' },
    svg: () => '', hints: () => '', HEART: '',
  });
}

function desktopPage(os, can, on) {
  const c = settingsContext(os, can, on);
  vm.runInContext(fn(app, 'startAtLoginNote') + fn(app, 'pageSettings'), c);
  return c.pageSettings();
}

function actContext(answer) {
  const c = settingsContext('linux', true, false);
  c.calls = [];
  c.toasts = [];
  c.invoke = async (command, args) => {
    c.calls.push({ command, args });
    if (answer instanceof Error) throw answer;
    return answer;
  };
  c.render = () => {};
  c.toast = (msg) => c.toasts.push(msg);
  c.fail = (e) => c.toasts.push(String(e.message || e));
  vm.runInContext('let settingsSaving = Promise.resolve();' + fn(app, 'applyEasySkip') + fn(app, 'saveSettings') + fn(app, 'startAtLogin') + fn(app, 'act'), c);
  return c;
}

const toggle = { dataset: { act: 'toggle', k: 'start_at_login' } };

test('desktop shows Start with your PC with the saved value', () => {
  assert.match(desktopPage('windows', true, false), /aria-checked="false" aria-label="Start with your PC" data-act="toggle" data-k="start_at_login"/);
  assert.match(desktopPage('windows', true, true), /aria-checked="true" aria-label="Start with your PC" data-act="toggle" data-k="start_at_login"/);
});

test('desktop leaves Start with your PC out where the system has no way to do it', () => {
  assert.doesNotMatch(desktopPage('macos', false, false), /start_at_login/);
});

test('a Mac says the window opens at login', () => {
  assert.match(desktopPage('macos', true, false), /opens when you log in/);
});

test('Windows and Linux say the launcher waits by the clock', () => {
  assert.match(desktopPage('linux', true, false), /waits by the clock/);
});

test('Big Picture shows the same switch', () => {
  const c = settingsContext('linux', true, true);
  vm.runInContext(fn(app, 'startAtLoginNote') + fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="start_at_login" data-sw="1"><span class="k">Start with your PC/);
});

test('the switch goes through its own command, not save_settings', async () => {
  const c = actContext(true);
  await c.act(toggle);
  assert.deepEqual(c.calls.map((x) => x.command), ['set_start_at_login']);
  assert.equal(c.calls[0].args.on, true);
});

test('the switch shows what the system took', async () => {
  const c = actContext(true);
  await c.act(toggle);
  assert.equal(c.S.settings.start_at_login, true);
});

test('a desktop that says no leaves the switch off and says so', async () => {
  const c = actContext(false);
  await c.act(toggle);
  assert.equal(c.S.settings.start_at_login, false);
  assert.equal(c.toasts.length, 1);
});

test('an error leaves the switch as it was', async () => {
  const c = actContext(new Error('Couldn\'t find your settings folder.'));
  await c.act(toggle);
  assert.equal(c.S.settings.start_at_login, false);
  assert.deepEqual(c.toasts, ['Couldn\'t find your settings folder.']);
});
