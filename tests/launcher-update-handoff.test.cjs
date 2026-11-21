const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => source.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// The core can finish by opening a download link, without an in-place update or progress events.
function updating(os) {
  let resolve, reject;
  const pending = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  const classes = new Set();
  const nodes = Object.fromEntries(['.uw-go', '[data-l]', '[data-t]', '[data-s]'].map((name) => [name, { textContent: '', style: { removeProperty() {} } }]));
  const win = { dataset: {}, classList: { add: (x) => classes.add(x), remove: (x) => classes.delete(x), contains: (x) => classes.has(x) } };
  const errors = [];
  const c = vm.createContext({
    S: { os }, invoke: () => pending, $: (selector) => nodes[selector],
    document: { body: { classList: { contains: () => false } } },
    t: (s) => s, verName: (s) => s, tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, key) => values[key]),
    blocked: () => false, cantReplace: () => false, fail: (error) => errors.push(error),
    lookForLauncher() {}, toast() {},
  });
  vm.runInContext(['selfUpdate', 'updWinStart', 'updWinReset'].map(fn).join('\n'), c);
  c.updWinStart(win, { version: 'next' });
  const done = c.selfUpdate(win);
  return { c, win, nodes, errors, resolve, reject, done, busy: () => classes.has('busy') };
}

test('the update window stays busy while the core is still working', async () => {
  const c = updating('linux');
  await Promise.resolve();
  assert.equal(c.busy(), true);
  assert.equal(c.c.S.selfUpdating, true);
  assert.equal(c.nodes['[data-l]'].textContent, '0%');
  c.resolve();
  await c.done;
});

test('the Linux external download handoff makes the update window usable again', async () => {
  const c = updating('linux');
  c.resolve();
  await c.done;
  assert.equal(c.busy(), false);
  assert.equal(c.c.S.selfUpdating, false);
  assert.equal(c.nodes['[data-l]'].textContent, 'Update Now');
});

test('the Mac external download handoff makes the update window usable again', async () => {
  const c = updating('macos');
  c.resolve();
  await c.done;
  assert.equal(c.busy(), false);
  assert.equal(c.nodes['[data-t]'].textContent, 'Update to next');
});

test('a failed update still resets the window and reports its error', async () => {
  const c = updating('windows');
  const error = new Error('The download failed');
  c.reject(error);
  await c.done;
  assert.equal(c.busy(), false);
  assert.equal(c.c.S.selfUpdating, false);
  assert.deepEqual(c.errors, [error]);
  assert.equal(c.nodes['[data-l]'].textContent, 'Update Now');
});
