const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];

function settingsContext(os) {
  const c = vm.createContext({
    S: { os, settings: {}, libs: [], library: {}, owned: {}, steam: false },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s, sub) => `${s}|${sub}`,
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '', clockRow: () => '', startAtLoginNote: () => '',
  });
  c.window = c;
  c.monitorPicked = () => ''; c.monitorName = () => '';
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  return c;
}

test('Big Picture on Android leaves out the PC-only settings', () => {
  const html = settingsContext('android').pageSettings();
  for (const gone of ['Leave Big Picture mode', 'Start in Big Picture mode', 'Close to tray', 'Discord status', 'Controller as a Mouse', 'Monitor']) {
    assert.doesNotMatch(html, new RegExp(gone), gone);
  }
  assert.match(html, /Auto-Install/);
});

test('Big Picture on a PC keeps them', () => {
  const html = settingsContext('windows').pageSettings();
  for (const kept of ['Leave Big Picture mode', 'Start in Big Picture mode', 'Close to tray', 'Discord status', 'Controller as a Mouse', 'Monitor']) {
    assert.match(html, new RegExp(kept), kept);
  }
});

test('Android always starts in Big Picture mode', () => {
  const c = vm.createContext({ S: { os: 'android', settings: {}, desktopArg: true } });
  vm.runInContext(fn(app, 'startsInTV'), c);
  assert.equal(c.startsInTV(), true);
  c.S.os = 'windows';
  assert.equal(c.startsInTV(), false);
});
