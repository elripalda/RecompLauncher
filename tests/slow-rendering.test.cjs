const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
// Run the production monitor with a controlled frame clock, without launching Tauri.
function monitor({ os = 'linux', told = false } = {}) {
  let now = 100, warning = 0, modal = false;
  const classes = new Set(), listeners = new Map(), pending = [];
  const document = { hidden: false, body: { classList: { contains: c => classes.has(c) } }, querySelector: () => modal };
  const S = { os, settings: { ui: { slow_told: told } }, running: new Set() };
  const context = vm.createContext({
    S, document, window: {}, performance: { now: () => now },
    addEventListener: (name, fn) => listeners.set(name, fn),
    requestAnimationFrame: fn => pending.push(fn), tellSlow: () => warning++,
  });
  vm.runInContext(source.match(/^function watchSlow\([^]*?^}/m)[0], context);
  vm.runInContext('watchSlow()', context);
  function interact(interval) {
    listeners.get('pointerdown')?.();
    for (let t = 0; t < 560; t += interval) {
      now += interval;
      const callbacks = pending.splice(0);
      callbacks.forEach(fn => fn(now));
    }
    now += 600; // The player reads the next setup page before pressing Continue.
  }
  return { interact, classes, document, S, pending, listeners,
    warnings: () => warning, modal: value => { modal = value; } };
}

test('slow setup transitions prompt within a few seconds of active drawing', () => {
  const m = monitor();
  for (let i = 0; i < 8; i++) m.interact(60);
  assert.equal(m.warnings(), 1);
  for (let i = 0; i < 8; i++) m.interact(60);
  assert.equal(m.warnings(), 1);
});

test('normal 60fps and 30fps rendering do not prompt', () => {
  for (const interval of [16, 33]) {
    const m = monitor();
    for (let i = 0; i < 45; i++) m.interact(interval);
    assert.equal(m.warnings(), 0);
  }
});

test('a dialog defers the warning until later interaction', () => {
  const m = monitor(); m.modal(true);
  for (let i = 0; i < 8; i++) m.interact(60);
  assert.equal(m.warnings(), 0);
  m.modal(false);
  for (let i = 0; i < 8; i++) m.interact(60);
  assert.equal(m.warnings(), 1);
});

test('hidden windows, playing games and Big Picture do not start sampling', () => {
  for (const mode of ['hidden', 'playing', 'tv']) {
    const m = monitor();
    if (mode === 'hidden') m.document.hidden = true;
    if (mode === 'playing') m.S.running.add('game');
    if (mode === 'tv') m.classes.add('tv-on');
    m.interact(60);
    assert.equal(m.pending.length, 0);
    assert.equal(m.warnings(), 0);
  }
});

test('lighter mode and previously acknowledged warnings remain quiet', () => {
  for (const config of [{ os: 'linux', told: true }, { os: 'windows' }, { os: 'macos' }]) {
    const m = monitor(config);
    if (config.os === 'macos') m.classes.add('lighter');
    for (let i = 0; i < 45; i++) m.interact(60);
    assert.equal(m.warnings(), 0);
  }
});
