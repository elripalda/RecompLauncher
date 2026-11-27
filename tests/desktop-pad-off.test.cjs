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
function desktop(padDesktop, os = 'windows') {
  const got = [];
  const c = vm.createContext({
    S: { os, settings: { pad_desktop: padDesktop } },
    android: () => os === 'android',
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

test('RB used as a mouse click does nothing on the desktop view with Controller as a Mouse on', () => {
  const c = desktop(false);
  c.pad('rb');
  assert.deepEqual(c.got, []);
});

test('A and the d-pad reach nothing on the desktop view with Controller as a Mouse on', () => {
  const c = desktop(false);
  c.pad('down');
  c.pad('a');
  assert.deepEqual(c.got, []);
});

test('holding Menu still opens Big Picture mode with Controller as a Mouse on', () => {
  const c = desktop(false);
  c.pad('menu-hold');
  assert.deepEqual(c.got, ['big-picture']);
});

test('the controller moves around the desktop view as before when the setting was never changed', () => {
  const c = desktop(undefined);
  c.pad('rb');
  assert.deepEqual(c.got, ['files:rb', 'first-run:rb', 'desktop:rb']);
});

test('Settings → Controllers has Controller as a Mouse, off unless turned on', () => {
  assert.match(app, /sw\('pad_desktop', st\.pad_desktop === false, t\('Controller as a Mouse'\)\)/);
});

test('turning Controller as a Mouse on with the controller takes its highlight away', () => {
  assert.match(app, /if \(k === 'pad_desktop'\) \{ await saveSettings\(\{ pad_desktop: S\.settings\.pad_desktop === false \}\); if \(S\.settings\.pad_desktop === false\) padLetGo\(\); return; \}/);
});

test('Big Picture settings have Controller as a Mouse too, off unless turned on', () => {
  assert.match(tv, /sw\('pad_desktop', t\('Controller as a Mouse'\), t\('(?:[^'\\]|\\.)+'\), S\.settings\.pad_desktop === false\)/);
});

// The desktop view's controller code from app.js, run as tests/desktop-pad.test.cjs runs it.
function desktopView() {
  const start = app.indexOf('// ---------- a controller on the desktop view ----------');
  const end = app.indexOf('// The window has no system title bar', start);
  const classes = () => { const v = new Set(); return { add: (x) => v.add(x), remove: (x) => v.delete(x), contains: (x) => v.has(x) }; };
  const body = { classList: classes() };
  const document = { body, activeElement: body, querySelectorAll: () => [], addEventListener: () => {} };
  const button = {
    id: 'switch', tagName: 'BUTTON', dataset: {}, classList: classes(), isConnected: true,
    getAttribute: () => null, closest: () => null, contains: (el) => el === button,
    matches: (s) => s === '.btn.main' || s.includes('[aria-selected'),
    getBoundingClientRect: () => ({ left: 0, top: 0, right: 100, bottom: 40, width: 100, height: 40 }),
    focus: () => { document.activeElement = button; }, blur: () => { document.activeElement = body; },
  };
  const root = { id: 'app', querySelectorAll: () => [button], contains: (el) => el === button };
  const main = { contains: (el) => el === button, getBoundingClientRect: () => ({ top: 0, bottom: 500 }) };
  const window = { addEventListener: () => {} };
  const c = vm.createContext({
    window, document, SU: {}, S: { back: [], view: 'settings' },
    $: (s) => (s === '#app' ? root : s === '#main' ? main : null), $$: () => [], MutationObserver: class { observe() {} },
    getComputedStyle: () => ({ visibility: 'visible' }), performance: { now: () => 1000 },
    calm: () => true, setTimeout: () => {}, requestAnimationFrame: () => {},
  });
  vm.runInContext(app.slice(start, end), c);
  return { c, window, document, body, button };
}

test('turning Controller as a Mouse on leaves nothing highlighted or focused on the desktop view', () => {
  const d = desktopView();
  d.window.deskPad('down');
  assert.equal(d.document.activeElement, d.button);
  d.c.padLetGo();
  assert.equal(d.button.classList.contains('pad-focus'), false);
  assert.equal(d.body.classList.contains('pad-nav'), false);
  assert.equal(d.document.activeElement, d.body);
});

test('on Android any controller button switches from the phone layout to Big Picture mode', () => {
  for (const b of ['a', 'down', 'rb', 'menu-hold']) {
    const c = desktop(undefined, 'android');
    c.pad(b);
    assert.deepEqual(c.got, ['big-picture'], b);
  }
});
