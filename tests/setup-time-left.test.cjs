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

test('2 minutes with no progress: taking longer than expected, and the time left once it moves', () => {
  const c = estimate();
  let s = c.stepEvent(undefined, { pct: 0, text: 'Compiling' }, 0);
  s = c.stepEvent(s, { pct: 0.25, text: 'Compiling' }, MIN);
  // The same pct again isn't progress.
  s = c.stepEvent(s, { pct: 0.25, text: 'Compiling' }, 2 * MIN);
  assert.equal(c.stepLeft(s, MIN + 119000), '9 min left');
  assert.equal(c.stepLeft(s, 3 * MIN), 'Taking longer than expected');
  s = c.stepEvent(s, { pct: 0.5, text: 'Compiling' }, 4 * MIN);
  assert.equal(c.stepLeft(s, 4 * MIN), '4 min left');
});

// ---------- the desktop setup window ----------
// The window's box, as much of it as the run pane uses: each step's line is an object the test reads.
function desktop() {
  const c = base();
  const box = { dataset: {}, classList: { add() {}, remove() {} }, html: '', firstChild: null, lines: [] };
  Object.defineProperty(box, 'innerHTML', {
    get: () => box.html,
    set: (h) => {
      box.html = h; box.firstChild = h ? {} : null;
      box.lines = [...h.matchAll(/<div class="k"><b>[^<]*<\/b><span>([^<]*)<\/span>/g)].map((m) => ({ textContent: m[1] }));
    },
  });
  const el = { box, remove() { el.gone = true; } };
  Object.assign(c, {
    SU: { id: 'g', el, stage: 'run', step: 'extras', steps: {}, q: [], at: 0, path: '/games/game.iso', extras: true, kept: false, upd: false },
    SU_ORDER: ['read', 'verify', 'extras', 'setup', 'done'],
    suDrive: { start() {}, stop() { c.driveStopped = true; } },
    document: { activeElement: null },
    CHECK: '', XMARK: '',
    $: (sel, r) => {
      if (sel === '.modal') return box;
      if (sel === '.steps') return box.html.includes('class="steps"') ? {} : null;
      if (sel === '.k span') return r.line;
      if (sel === '.su-r') return { set innerHTML(h) { box.innerHTML = h; } };
      if (sel === '.su-bar i') return { style: {} };
      if (sel === '.su-now') return { textContent: '' };
      return null;
    },
    $$: (sel) => {
      if (sel === '.step') return box.lines.map((l) => ({ line: l }));
      if (sel === '.su-r .step .k span') return box.lines;
      return [];
    },
  });
  vm.createContext(c);
  vm.runInContext([
    left, line(app, /^const SU_TICK = .*$/m), line(app, /^function suTickStop\(\) \{.*\}$/m),
    fn(app, 'suTick'), fn(app, 'suLines'), fn(app, 'suDraw'), fn(app, 'suPump'), fn(app, 'suBackground'), fn(app, 'suAct'),
  ].join('\n'), c);
  return c;
}
const send = (c, q, ev) => { c[q].q.push(Object.assign({ step: 'extras', extras: true, text: 'Compiling' }, ev)); };
const extrasLine = (c) => c.SU.el.box.lines[2].textContent;

test('the desktop run pane has Run in Background and the time left on the running step', () => {
  const c = desktop();
  send(c, 'SU', { pct: 0 }); c.suPump();
  const html = c.SU.el.box.html;
  assert.match(html, /<button type="button" class="btn big" data-su="bg">Run in Background<\/button>/);
  assert.equal(extrasLine(c), 'Compiling');
  c.clock = MIN; send(c, 'SU', { pct: 0.25 }); c.suPump();
  assert.equal(extrasLine(c), 'Compiling · 3 min left');
});

test('desktop: a quiet step says it is taking longer, without an event, and the time left comes back', () => {
  const c = desktop();
  send(c, 'SU', { pct: 0 }); c.suPump();
  c.clock = MIN; send(c, 'SU', { pct: 0.25 }); c.suPump();
  assert.equal(c.ticks.filter(Boolean).length, 1, 'one check while it runs');
  c.clock = MIN + 110000; c.tick();
  assert.match(extrasLine(c), /· \d+ min left$/);
  c.clock = 3 * MIN; c.tick();
  assert.equal(extrasLine(c), 'Compiling · Taking longer than expected');
  c.clock = 4 * MIN; send(c, 'SU', { pct: 0.5 }); c.suPump();
  assert.equal(extrasLine(c), 'Compiling · 4 min left');
});

test('Run in Background keeps setup going: the window closes, setup is not cancelled', async () => {
  const c = desktop();
  send(c, 'SU', { pct: 0.1 }); c.suPump();
  await c.suAct({ dataset: { su: 'bg' } });
  assert.deepEqual(c.closed, ['g']);
  assert.ok(!c.invoked.includes('setup_cancel'));
  assert.equal(c.SU.el, null);
  assert.ok(c.driveStopped);
  assert.equal(c.ticks.filter(Boolean).length, 0, 'the check stops with the window');
  assert.deepEqual(c.toasts, ['Setup for Test Game will continue in the background.']);
});

test('desktop: the check stops when the run does', () => {
  const c = desktop();
  send(c, 'SU', { pct: 0.1 }); c.suPump();
  c.SU.stage = 'error'; c.SU.err = 'x';
  c.tick();
  assert.equal(c.ticks.filter(Boolean).length, 0);
});

// ---------- Big Picture ----------
function bigPicture() {
  const c = base();
  const st = { el: null, append(e) { st.el = e; } };
  const mk = () => {
    const el = { className: '', dataset: {}, html: '', tks: [], pct: { textContent: '' } };
    Object.defineProperty(el, 'innerHTML', {
      get: () => el.html,
      set: (h) => {
        el.html = h;
        el.tks = [...h.matchAll(/<div class="tk[^"]*"><i><u[^>]*><\/u><\/i><b>[^<]*<\/b><span>([^<]*)<\/span>/g)].map((m) => ({ u: { style: {} }, span: { textContent: m[1] } }));
      },
    });
    return el;
  };
  Object.assign(c, {
    TS: { id: 'g', stage: 'run', step: 'extras', steps: {}, q: [], at: 0, path: '/games/game.iso', extras: true, kept: false, upd: false },
    TV: { on: true, sheet: null },
    stage: () => st, focused: () => null, focus: () => {}, glyph: () => '', closeSheet: () => {},
    document: { createElement: mk },
    $: (sel, r) => {
      if (sel === '.tv-setup:not(.out)') return r === st ? st.el : null;
      if (sel === '.track') return r.html.includes('class="track"') ? {} : null;
      if (sel === '.pct') return r.pct;
      if (sel === 'u') return r.u;
      if (sel === 'span') return r.span;
      return null;
    },
    $$: (sel, r) => {
      if (sel === '.tk') return r.tks;
      if (sel === '.tk span') return r.tks.map((x) => x.span);
      return [];
    },
  });
  vm.createContext(c);
  vm.runInContext([
    left, line(tv, /^ {2}const TS_ORDER = .*$/m), line(tv, /^ {2}const TS_TICK = .*$/m),
    fn(tv, 'tsLines', '  '), fn(tv, 'tsTick', '  '), fn(tv, 'tsDraw', '  '), fn(tv, 'tsPump', '  '),
  ].join('\n'), c);
  c.line = () => st.el.tks[2].span.textContent;
  return c;
}

test('Big Picture: the time left, taking longer than expected when quiet, and back when it moves', () => {
  const c = bigPicture();
  send(c, 'TS', { pct: 0 }); c.tsPump();
  assert.match(c.stage().el.html, /Run in Background/);
  assert.equal(c.line(), 'Compiling');
  c.clock = MIN; send(c, 'TS', { pct: 0.25 }); c.tsPump();
  assert.equal(c.line(), 'Compiling · 3 min left');
  c.clock = 3 * MIN; c.tick();
  assert.equal(c.line(), 'Compiling · Taking longer than expected');
  c.clock = 4 * MIN; send(c, 'TS', { pct: 0.5 }); c.tsPump();
  assert.equal(c.line(), 'Compiling · 4 min left');
  // Leaving Big Picture's sheet stops the check.
  c.TV.sheet.onClose();
  c.tick();
  assert.equal(c.ticks.filter(Boolean).length, 0);
});

test('Taking longer than expected is translated in every language', () => {
  const dir = path.join(__dirname, '../ui/i18n');
  for (const f of fs.readdirSync(dir)) {
    const tr = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'))['Taking longer than expected'];
    assert.ok(tr, f);
    if (f !== 'en-GB.json') assert.notEqual(tr, 'Taking longer than expected', f);
  }
});
