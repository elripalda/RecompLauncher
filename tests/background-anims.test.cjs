const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
// The focus/background block as app.js has it, from windowInFront to its first markWindowFocus().
const block = app.match(/^const windowInFront = [^]*?^markWindowFocus\(\);$/m)[0];

// A CSS animation as getAnimations() hands it out: endTime is Infinity for one that loops.
function anim(endTime) {
  const a = { playState: 'paused', finished: 0, effect: { getComputedTiming: () => ({ endTime }) } };
  a.finish = () => { a.finished++; a.playState = 'finished'; };
  return a;
}

// Runs the block with a fake window: focus and visibility, events, timers and one MutationObserver.
function launcher({ focused = true } = {}) {
  const classes = new Set();
  const body = { classList: {
    contains: c => classes.has(c),
    toggle: (c, on) => (on ? classes.add(c) : classes.delete(c))
  } };
  const listeners = {};
  const timers = [];
  const w = { focused, anims: [], observer: null, timers };
  const document = {
    hidden: false, body,
    hasFocus: () => w.focused,
    getAnimations: () => w.anims,
    addEventListener: (e, f) => { listeners[e] = f; }
  };
  class MutationObserver {
    constructor(cb) { this.cb = cb; this.watching = null; w.observer = this; }
    observe(target, opts) { this.watching = { target, opts }; }
    disconnect() { this.watching = null; }
  }
  const c = vm.createContext({
    document, MutationObserver,
    addEventListener: (e, f) => { listeners[e] = f; },
    setTimeout: f => timers.push(f),
    setInterval: () => {}
  });
  vm.runInContext(block, c);
  // The away event in app.js toggles the class, then calls markStill.
  w.markStill = c.markStill;
  w.body = body;
  w.blur = () => { w.focused = false; listeners.blur(); };
  w.focus = () => { w.focused = true; listeners.focus(); };
  // Something was added to the page: the observer fires as a browser would, if it's connected.
  w.addContent = () => { if (w.observer.watching) w.observer.cb([{ type: 'childList' }]); };
  w.tick = () => { const due = timers.splice(0); due.forEach(f => f()); };
  return w;
}

test('a finite animation is put at its end while the window is in the background', () => {
  const w = launcher();
  const a = anim(300);
  w.anims = [a];
  w.blur();
  assert.equal(a.finished, 1);
});

test('an endless animation stays paused while the window is in the background', () => {
  const w = launcher();
  const spinner = anim(Infinity);
  w.anims = [spinner];
  w.blur();
  assert.equal(spinner.finished, 0);
});

test('an animation that already finished is left alone', () => {
  const w = launcher();
  const a = anim(300);
  a.playState = 'finished';
  w.anims = [a];
  w.blur();
  assert.equal(a.finished, 0);
});

test('nothing is touched while the window has focus', () => {
  const w = launcher();
  const a = anim(300);
  w.anims = [a];
  w.focus();
  w.addContent();
  w.tick();
  assert.equal(a.finished, 0);
});

test('a batch queued in the background does nothing if focus is back before it runs', () => {
  const w = launcher();
  w.blur();
  w.addContent();
  const a = anim(300);
  w.anims = [a];
  w.focus();
  w.tick();
  assert.equal(a.finished, 0);
});

test('the observer watches the page while in the background and stops when the window is back', () => {
  const w = launcher();
  assert.equal(w.observer.watching, null);
  w.blur();
  assert.equal(w.observer.watching.target, w.body);
  assert.deepEqual({ ...w.observer.watching.opts }, { childList: true, subtree: true });
  w.focus();
  assert.equal(w.observer.watching, null);
});

test('a launcher that starts in the background watches from the start', () => {
  const w = launcher({ focused: false });
  assert.ok(w.observer.watching);
});

test('content added while in the background is settled once per batch', () => {
  const w = launcher();
  w.blur();
  const a = anim(300);
  w.anims = [a];
  w.addContent();
  w.addContent();
  w.addContent();
  assert.equal(w.timers.length, 1, 'three mutations, one settle queued');
  assert.equal(a.finished, 0, 'nothing settles before the tick');
  w.tick();
  assert.equal(a.finished, 1);
});

test('after a batch settles, the next mutation queues a new one', () => {
  const w = launcher();
  w.blur();
  w.addContent();
  w.tick();
  const b = anim(200);
  w.anims = [b];
  w.addContent();
  assert.equal(w.timers.length, 1);
  w.tick();
  assert.equal(b.finished, 1);
});

test('minimised (away) settles and watches even with focus, and stops once it is back', () => {
  const w = launcher();
  const a = anim(300);
  w.anims = [a];
  w.body.classList.toggle('away', true);
  w.markStill();
  assert.equal(a.finished, 1);
  assert.ok(w.observer.watching);
  w.body.classList.toggle('away', false);
  w.markStill();
  assert.equal(w.observer.watching, null);
});

test('the away event calls markStill after it sets the class', () => {
  assert.match(app, /classList\.toggle\('away', S\.away\); markStill\(\);/);
});
