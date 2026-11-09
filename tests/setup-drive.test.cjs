const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const code = fs.readFileSync(path.join(__dirname, '../ui/setup-drive.js'), 'utf8');

function fixture(random = .9) {
  const listeners = () => {
    const events = new Map();
    return {
      addEventListener: (name, fn) => events.set(name, fn),
      removeEventListener: (name) => events.delete(name),
      fire: (name) => events.get(name)?.(),
      count: () => events.size,
    };
  };
  const doc = { ...listeners(), hidden: false }, motion = { ...listeners(), matches: false };
  const pending = new Map(); let nextId = 0;
  const ctx = vm.createContext({
    window: { matchMedia: () => motion }, document: doc,
    Math: Object.assign(Object.create(Math), { random: () => random }),
    setTimeout: (fn, delay) => { const id = ++nextId; pending.set(id, { fn, delay }); return id; },
    clearTimeout: (id) => pending.delete(id),
  });
  vm.runInContext(code, ctx);
  const element = () => {
    const classes = new Set();
    return { isConnected: true, classes, classList: {
      add: (c) => classes.add(c), remove: (c) => classes.delete(c),
      toggle: (c, on) => on ? classes.add(c) : classes.delete(c),
    } };
  };
  const light = element(), drive = ctx.createSetupDriveLight();
  const delay = () => pending.values().next().value?.delay;
  const advance = () => { assert.equal(pending.size, 1); const [id, task] = pending.entries().next().value; pending.delete(id); task.fn(); };
  return { drive, light, element, doc, motion, pending, delay, advance };
}

test('rapid flickers and long glows both alternate with short dark gaps', () => {
  const fast = fixture(.1), slow = fixture(.9);
  for (const f of [fast, slow]) {
    assert.equal(f.pending.size, 0);
    f.drive.start(f.light);
    assert.equal(f.light.classes.has('on'), true);
  }
  assert.ok(fast.delay() >= 45 && fast.delay() <= 95);
  assert.ok(slow.delay() >= 650 && slow.delay() <= 1600);
  for (const f of [fast, slow]) {
    for (let n = 0; n < 12; n++) {
      f.advance();
      if (!f.light.classes.has('on')) assert.ok(f.delay() >= 40 && f.delay() <= 450);
    }
  }
});

test('progress redraws keep one timer; stopping clears the light and listeners', () => {
  const f = fixture();
  f.drive.start(f.light); f.advance();
  const task = [...f.pending.values()][0];
  f.drive.start(f.light);
  assert.equal([...f.pending.values()][0], task);
  assert.equal(f.light.classes.has('on'), false);
  f.drive.stop(); f.drive.stop();
  assert.equal(f.pending.size, 0);
  assert.equal(f.light.classes.has('on'), false);
  assert.equal(f.doc.count() + f.motion.count(), 0);
});

test('hidden windows pause and resume; reduced motion stays steadily lit', () => {
  const f = fixture();
  f.drive.start(f.light);
  f.doc.hidden = true; f.doc.fire('visibilitychange');
  assert.equal(f.pending.size, 0);
  assert.equal(f.light.classes.has('on'), false);
  f.doc.hidden = false; f.doc.fire('visibilitychange');
  assert.equal(f.pending.size, 1);
  f.motion.matches = true; f.motion.fire('change');
  assert.equal(f.pending.size, 0);
  assert.equal(f.light.classes.has('on'), true);
  f.motion.matches = false; f.motion.fire('change');
  assert.equal(f.pending.size, 1);
});

test('replaced and removed elements do not leave activity loops behind', () => {
  const f = fixture(), replacement = f.element();
  f.drive.start(f.light); f.drive.start(replacement);
  assert.equal(f.light.classes.has('on'), false);
  assert.equal(f.pending.size, 1);
  replacement.isConnected = false; f.advance();
  assert.equal(f.pending.size, 0);
  assert.equal(replacement.classes.has('on'), false);
  assert.equal(f.doc.count() + f.motion.count(), 0);
});
