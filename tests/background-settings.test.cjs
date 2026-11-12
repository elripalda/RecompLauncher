const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];

function settingsContext(os, on) {
  return vm.createContext({
    S: {
      os, settings: { close_to_tray: on }, libs: [], library: {}, owned: {},
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab: 'general', view: 'settings',
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['general', 'General']], SET_ICONS: {},
    okMark: () => '', portableRow: () => '', geRow: () => '',
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '',
  });
}

for (const os of ['windows', 'linux', 'macos']) {
  test(`desktop exposes Run in background with the saved value on ${os}`, () => {
    const c = settingsContext(os, false);
    vm.runInContext(fn(app, 'pageSettings'), c);
    const off = c.pageSettings();
    assert.match(off, /aria-checked="false" aria-label="Run in background" data-act="toggle" data-k="close_to_tray"/);
    assert.match(off, /Quit always exits/);
    c.S.settings.close_to_tray = true;
    assert.match(c.pageSettings(), /aria-checked="true" aria-label="Run in background" data-act="toggle" data-k="close_to_tray"/);
  });
}

test('Big Picture exposes the same background preference', () => {
  const c = settingsContext('linux', false);
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="close_to_tray" data-sw="0"><span class="k">Run in background/);
  c.S.settings.close_to_tray = true;
  assert.match(c.pageSettings(), /data-toggle="close_to_tray" data-sw="1"><span class="k">Run in background/);
});

test('the background toggle saves both choices through the existing settings command', async () => {
  const c = settingsContext('macos', false);
  const saved = [];
  c.invoke = async (command, args) => saved.push({ command, on: args.settings.close_to_tray });
  c.render = () => {};
  c.fail = (error) => { throw error; };
  vm.runInContext('let settingsSaving = Promise.resolve();' + fn(app, 'saveSettings') + fn(app, 'act'), c);
  const toggle = { dataset: { act: 'toggle', k: 'close_to_tray' } };
  await c.act(toggle);
  await c.act(toggle);
  assert.deepEqual(saved, [
    { command: 'save_settings', on: true },
    { command: 'save_settings', on: false },
  ]);
});
