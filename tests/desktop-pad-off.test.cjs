const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^  (?:async )?function ${name}\\([^]*?^  }`, 'm'))[0];

// The desktop view (Big Picture closed), with every handler a controller press could reach noting
// that it got it.
function desktop(padDesktop) {
  const got = [];
  const c = vm.createContext({
    S: { settings: { pad_desktop: padDesktop } },
    TV: { on: false, hinted: true },
    lastPad: { b: '', t: 0 },
    performance: { now: () => 10000 },
    document: { hasFocus: () => true, querySelector: () => null },
    window: {
      fbPad: (b) => { got.push(`files:${b}`); return false; },
      oobPad: (b) => { got.push(`first-run:${b}`); return false; },
      deskPad: (b) => { got.push(`desktop:${b}`); return true; },
    },
    holdOpensTV: () => true,
    enter: () => got.push('big-picture'),
    toast: () => {}, t: (s) => s,
    got,
  });
  vm.runInContext(fn(tv, 'pad'), c);
  return c;
}

test('RB used as a mouse click does nothing on the desktop view with the controller off there', () => {
  const c = desktop(false);
  c.pad('rb');
  assert.deepEqual(c.got, []);
});

test('A and the d-pad reach nothing on the desktop view with the controller off there', () => {
  const c = desktop(false);
  c.pad('down');
  c.pad('a');
  assert.deepEqual(c.got, []);
});

test('holding Menu still opens Big Picture mode with the controller off on the desktop view', () => {
  const c = desktop(false);
  c.pad('menu-hold');
  assert.deepEqual(c.got, ['big-picture']);
});

test('the controller moves around the desktop view as before when the setting was never changed', () => {
  const c = desktop(undefined);
  c.pad('rb');
  assert.deepEqual(c.got, ['files:rb', 'first-run:rb', 'desktop:rb']);
});

test('Settings → Controllers has the switch, on unless turned off', () => {
  assert.match(app, /sw\('pad_desktop', st\.pad_desktop !== false, t\('Use the Controller Outside Big Picture'\)\)/);
});

test('turning it off with the controller takes its highlight away', () => {
  assert.match(app, /if \(k === 'pad_desktop'\) \{ await saveSettings\(\{ pad_desktop: S\.settings\.pad_desktop === false \}\); if \(S\.settings\.pad_desktop === false\) padLetGo\(\); return; \}/);
});
