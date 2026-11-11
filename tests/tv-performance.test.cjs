const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
// Exercise the real event listener and navigation code, with a small DOM stand-in. The hooks
// exist only in this test's copy of the script.
function fixture() {
  const events = {}, queries = [];
  const classes = () => ({ add() {}, remove() {}, contains: () => false });
  const root = { addEventListener() {}, classList: classes() };
  const hero = { innerHTML: '', offsetWidth: 600, classList: classes() };
  const heading = { dataset: {}, textContent: 'Game', style: {}, classList: classes(), clientWidth: 600, scrollWidth: 80 };
  const row = { classList: { contains: (name) => name === 'tv-row' } };
  const tile = (id) => ({
    dataset: { hero: id }, classList: classes(),
    closest: (sel) => sel === '.tv-row, .tv-acts, .tv-shots, .tv-nav' ? row : null,
  });
  const first = tile('first'), second = tile('second');
  let current = first, now = 1000, wholePageScans = 0;
  const c = vm.createContext({
    window: {}, navigator: {}, performance: { now: () => now },
    document: { addEventListener() {}, hasFocus: () => true, body: { classList: classes() } },
    S: { settings: { ui: {} }, library: {}, padKind: 'xbox' },
    T: { event: { listen: (name, fn) => { events[name] = fn; } } },
    N_: (s) => s, t: (s) => s, esc: String, shortName: (s) => s,
    getComputedStyle: () => ({ fontSize: '84px' }),
    addEventListener() {}, setTimeout() {}, clearTimeout() {}, setInterval() {},
    $: (sel) => {
      if (sel === '#tv') return root;
      if (sel === '#tv-hero') return hero;
      if (sel.includes('[data-f].f')) return current;
      return null;
    },
    $$: (sel, scope) => {
      queries.push({ sel, scope });
      if (scope === hero) return sel === '.tv-h1' ? [heading] : [];
      if (sel === '[data-f]' && scope === row) return [first, second];
      if (scope === root && ['.tv-h1', '.tv-card .cap b', '.tv-name'].includes(sel)) {
        wholePageScans++;
        return Array.from({ length: 1000 }, () => heading);
      }
      return [];
    },
    byId: (id) => ({ id, name: id }),
  });
  vm.runInContext(source.replace(/\}\)\(\);\s*$/, `
    window.testTV = { TV, pad, focus, move };
    // Isolate title fitting and navigation from unrelated image and scrolling work.
    heroText = (a) => '<h1 class="tv-h1">' + a.name + '</h1>';
    swapBg = () => {}; setTone = () => {};
    keepInView = (el) => { currentFocus(el); };
  })();`), Object.assign(c, { currentFocus: (el) => { current = el; } }));
  const api = c.window.testTV;
  api.TV.on = true;
  return { api, events, root, hero, queries, first, second,
    setTime: (t) => { now = t; },
    scans: () => wholePageScans, current: () => current };
}

test('native controller presses queued during a UI stall are all delivered', () => {
  const f = fixture(), received = [];
  f.api.TV.sheet = { keys: (b) => { received.push(b); return true; } };
  // The core sent these at separate times; a blocked web view receives the batch at once.
  for (let i = 0; i < 4; i++) f.events.pad({ payload: { b: 'right', kind: 'xbox' } });
  assert.deepEqual(received, ['right', 'right', 'right', 'right']);
  f.events.pad({ payload: 'right' }); // older core payloads follow the same path
  assert.equal(received.length, 5);
});

test('the polled fallback still suppresses simultaneous duplicate controller reports', () => {
  const f = fixture(), received = [];
  f.api.TV.sheet = { keys: (b) => { received.push(b); return true; } };
  f.api.pad('right', 'xbox');
  f.api.pad('right', 'ps');
  f.setTime(1120);
  f.api.pad('right', 'xbox');
  assert.deepEqual(received, ['right', 'right']);
});

test('moving between Home games fits only the changed hero, independent of library size', () => {
  const f = fixture();
  f.api.TV.hero = 'first';
  f.api.focus(f.second);
  assert.equal(f.api.TV.hero, 'second');
  assert.equal(f.scans(), 0, 'the thousand unchanged game titles must not be measured');
  assert.ok(f.queries.some((q) => q.sel === '.tv-h1' && q.scope === f.hero), 'the new hero title must still be fitted');
});

test('horizontal row navigation avoids querying the whole page', () => {
  const f = fixture();
  f.api.TV.view = 'library';
  f.api.move('right');
  assert.equal(f.current(), f.second);
  assert.equal(f.queries.some((q) => q.sel === '[data-f]' && q.scope !== f.first.closest('.tv-row, .tv-acts, .tv-shots, .tv-nav')), false);
});
