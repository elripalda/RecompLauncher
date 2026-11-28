const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];

function settingsContext(os, touch = false) {
  const c = vm.createContext({
    S: { os, settings: {}, libs: [], library: {}, owned: {}, steam: false },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s, sub) => `${s}|${sub}`,
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '', clockRow: () => '', hideNameItem: () => '', startAtLoginNote: () => '',
  });
  c.window = c;
  c.monitorPicked = () => ''; c.monitorName = () => '';
  c.touchHere = () => touch;
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

test('Big Picture on an Android phone leaves to the phone layout', () => {
  const html = settingsContext('android', true).pageSettings();
  assert.match(html, /Leave Big Picture mode/);
  assert.match(html, /The phone layout, for playing with your fingers/);
  assert.doesNotMatch(html, /Back to the desktop view/);
});

test('Big Picture on a PC keeps them', () => {
  const html = settingsContext('windows').pageSettings();
  for (const kept of ['Leave Big Picture mode', 'Start in Big Picture mode', 'Close to tray', 'Discord status', 'Controller as a Mouse', 'Monitor']) {
    assert.match(html, new RegExp(kept), kept);
  }
});

function startContext(os, { pads = [], touch = 0 } = {}) {
  const c = vm.createContext({ S: { os, settings: {}, desktopArg: true }, navigator: { getGamepads: () => pads, maxTouchPoints: touch } });
  for (const name of ['startsInTV', 'padHere', 'touchHere']) vm.runInContext(fn(app, name), c);
  return c;
}

test('An Android phone starts in the phone layout, and in Big Picture mode with a controller', () => {
  assert.equal(startContext('android', { touch: 5 }).startsInTV(), false);
  assert.equal(startContext('android', { touch: 5, pads: [null, { id: 'Xbox Wireless Controller' }] }).startsInTV(), true);
  assert.equal(startContext('android', { touch: 5, pads: [null, null] }).startsInTV(), false);
});

test('An Android TV (no touch screen) always starts in Big Picture mode', () => {
  assert.equal(startContext('android').startsInTV(), true);
});

test('A PC starts where its settings say, whatever is plugged in', () => {
  assert.equal(startContext('windows', { touch: 10, pads: [{ id: 'pad' }] }).startsInTV(), false);
});

test('a controller leaves the phone layout alone when Open with a controller is off', () => {
  // Both ways a controller opens Big Picture mode on Android ask the phone layout first.
  const press = tv.slice(tv.indexOf('if (!TV.on) {'), tv.indexOf('if (S.settings.pad_desktop === false)'));
  assert.match(press, /android\(\)\) \{ if \(!window\.phonePadTv \|\| window\.phonePadTv\(\)\) enter\(\); return; \}/);
  const plug = tv.slice(tv.indexOf("addEventListener('gamepadconnected'"), tv.indexOf("addEventListener('gamepaddisconnected'"));
  assert.match(plug, /\(window\.phonePadTv && !window\.phonePadTv\(\)\)\) return;/);
  // A phone's own Big Picture mode asks the same (tv.js pad()).
  assert.match(tv, /if \(b !== 'menu-hold' && \(!window\.phonePadTv \|\| window\.phonePadTv\(\)\)\) pb\.enter\(true\);/);
});
