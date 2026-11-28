const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];
const line = (src, re) => src.match(re)[0];
// took, STEP_QUIET, stepLeft and stepEvent, as app.js has them.
const left = app.slice(app.indexOf('const took = '), app.indexOf('// (0.91) Each download'));
const MIN = 60000;

// What both screens need, with a clock the test moves and the intervals it can fire.
function base() {
  const c = {
    clock: 0, ticks: [], toasts: [], closed: [], invoked: [],
    t: (s) => s, N_: (s) => s, esc: String,
    tf: (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => v[k]),
    baseName: (p) => String(p).split('/').pop(),
    verifyLine: (s, d) => d,
    byId: () => ({ name: 'Test Game' }),
    S: { library: {}, setup: {}, view: 'home' },
    setupClosed: (id) => c.closed.push(id),
    invoke: (cmd, args) => { c.invoked.push(cmd); return Promise.resolve(); },
    toast: (s) => c.toasts.push(s),
    gameMediaHtml: () => '', media: (u) => u, cssUrl: (u) => u, thumbOf: (u) => u,
    profileSaid: () => {}, render: () => {}, refreshGame: () => {}, sfx: () => {},
  };
  c.Date = { now: () => c.clock };
  c.setInterval = (f) => { c.ticks.push(f); return c.ticks.length; };
  c.clearInterval = (n) => { if (n) c.ticks[n - 1] = null; };
  c.setTimeout = () => 0; c.clearTimeout = () => {};
  c.tick = () => c.ticks.forEach((f) => f && f());
  return c;
}

// ---------- the estimate itself ----------
function estimate() {
  const c = vm.createContext(base());
  vm.runInContext(left, c);
  return c;
}

test('half a step in 30 seconds leaves 30 seconds; a long step says minutes', () => {
  const c = estimate();
  assert.equal(c.stepLeft({ pct: 0.5, from: { at: 0, pct: 0 } }, 30000), '30 sec left');
  assert.equal(c.stepLeft({ pct: 0.25, from: { at: 0, pct: 0 } }, MIN), '3 min left');
  // The rate counts from `from`, not from 0.
  assert.equal(c.stepLeft({ pct: 0.6, from: { at: 0, pct: 0.4 } }, 10000), '20 sec left');
});

test('nothing is said in the first seconds, for a step with no progress, or a finished one', () => {
  const c = estimate();
  assert.equal(c.stepLeft({ pct: 0.5, from: { at: 0, pct: 0 } }, 2000), '');
  assert.equal(c.stepLeft({ pct: 0, from: { at: 0, pct: 0 } }, 10 * MIN), '');
  assert.equal(c.stepLeft({ pct: 1, from: { at: 0, pct: 0 } }, MIN), '');
  assert.equal(c.stepLeft(undefined), '');
});

test('0 → 1, then 0 → 0.5: the second run is timed on its own', () => {
  const c = estimate();
  let s = c.stepEvent(undefined, { pct: 0, text: 'Compiling' }, 0);
  s = c.stepEvent(s, { pct: 1, text: 'Compiling' }, 10 * MIN);
  s = c.stepEvent(s, { pct: 0, text: 'Compiling' }, 10 * MIN);
  s = c.stepEvent(s, { pct: 0.5, text: 'Compiling' }, 10 * MIN + 30000);
  assert.deepEqual({ ...s.from }, { at: 10 * MIN, pct: 0 });
  assert.equal(c.stepLeft(s, 10 * MIN + 30000), '30 sec left');
});

test('a new phase (its text changes) is timed on its own too', () => {
  const c = estimate();
  let s = c.stepEvent(undefined, { pct: 0, text: 'Downloading' }, 0);
  s = c.stepEvent(s, { pct: 0.5, text: 'Downloading' }, 10000);
  s = c.stepEvent(s, { pct: 0.5, text: 'Compiling' }, 10000);
  s = c.stepEvent(s, { pct: 0.6, text: 'Compiling' }, 110000);
  // 0.1 in 100 s: 0.4 left is about 7 minutes, not the minute the fast download would say.
  assert.equal(c.stepLeft(s, 110000), '7 min left');
});

test('the desktop setup window has Run in Background while it runs', () => {
  assert.match(app, /<button type="button" class="btn big" data-su="bg">\$\{esc\(t\('Run in Background'\)\)\}<\/button>/);
  assert.match(app, /case 'bg': return suBackground\(\);/);
});

test('desktop and Big Picture both put the time left on the running step', () => {
  assert.match(app, /const left = stepLeft\(st\[SU\.step\]\);/);
  assert.match(tv, /const left = stepLeft\(s\[TS\.step\]\);/);
});
