const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const start = app.indexOf('// ---------- a controller on the desktop view ----------');
const end = app.indexOf('// The window has no system title bar', start);
assert.ok(start >= 0 && end > start);

function desktop(focused = false) {
  const classes = () => {
    const values = new Set();
    return { add: x => values.add(x), remove: x => values.delete(x), contains: x => values.has(x) };
  };
  const listeners = {};
  const body = { classList: classes() };
  const document = { body, activeElement: body, querySelectorAll: () => [], addEventListener: (name, fn) => { listeners[name] = fn; } };
  let clicks = 0;
  const button = {
    id: 'play', tagName: 'BUTTON', dataset: {}, classList: classes(), isConnected: true,
    getAttribute: () => null, closest: () => null, contains: el => el === button,
    matches: selector => selector === '.btn.main' || selector.includes('[aria-selected'),
    getBoundingClientRect: () => ({ left: 0, top: 0, right: 100, bottom: 40, width: 100, height: 40 }),
    focus: () => { document.activeElement = button; }, click: () => { clicks++; },
  };
  const root = { id: 'app', querySelectorAll: () => [button], contains: el => el === button };
  const main = { contains: el => el === button, getBoundingClientRect: () => ({ top: 0, bottom: 500 }) };
  if (focused) document.activeElement = button;
  const windowListeners = {};
  const window = { addEventListener: (name, fn) => { windowListeners[name] = fn; } };
  const ctx = vm.createContext({
    window, document, SU: {}, S: { back: [], view: 'home' },
    $: selector => selector === '#app' ? root : selector === '#main' ? main : null,
    $$: () => [], MutationObserver: class { observe() {} },
    getComputedStyle: () => ({ visibility: 'visible' }), performance: { now: () => 1000 },
    calm: () => true, setTimeout: () => {}, requestAnimationFrame: () => {},
  });
  vm.runInContext(app.slice(start, end), ctx);
  return { window, windowListeners, document, body, button, listeners, get clicks() { return clicks; } };
}

test('A does not focus a desktop control before directional input', () => {
  const d = desktop();
  d.window.deskPad('a');
  assert.equal(d.document.activeElement, d.body);
});

test('A does not activate a pre-focused desktop control before directional input', () => {
  const d = desktop(true);
  d.window.deskPad('a');
  assert.equal(d.clicks, 0);
});

test('directional input focuses a desktop control and A activates it', () => {
  const d = desktop();
  d.window.deskPad('down');
  assert.equal(d.document.activeElement, d.button);
  d.window.deskPad('a');
  assert.equal(d.clicks, 1);
});

test('mouse movement disarms A until another direction is pressed', () => {
  const d = desktop();
  d.window.deskPad('down');
  d.listeners.mousemove({ movementX: 4, movementY: 0 });
  d.window.deskPad('a');
  assert.equal(d.clicks, 0);
  d.window.deskPad('down');
  d.window.deskPad('a');
  assert.equal(d.clicks, 1);
});

test('losing window focus disarms A until another direction is pressed', () => {
  const d = desktop();
  d.window.deskPad('down');
  d.window.deskPad('a');
  assert.equal(d.clicks, 1);
  d.windowListeners.blur();
  d.window.deskPad('a');
  assert.equal(d.clicks, 1);
  d.window.deskPad('down');
  d.window.deskPad('a');
  assert.equal(d.clicks, 2);
});

test('losing window focus removes the controller highlight', () => {
  const d = desktop();
  d.window.deskPad('down');
  assert.ok(d.button.classList.contains('pad-focus'));
  d.windowListeners.blur();
  assert.equal(d.body.classList.contains('pad-nav'), false);
  assert.equal(d.button.classList.contains('pad-focus'), false);
});

test('A can activate a dialog without first navigating the desktop', () => {
  const d = desktop(true);
  d.document.querySelectorAll = selector => selector === '.scrim, .oob' ? [d.dialog] : [];
  d.dialog = { id: 'dialog', querySelectorAll: () => [d.button], contains: el => el === d.button };
  d.window.deskPad('a');
  assert.equal(d.clicks, 1);
});
