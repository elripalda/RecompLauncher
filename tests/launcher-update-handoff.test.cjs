const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => source.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// Update Now in the update window, with the core's answer still to come: resolve('opened') is the
// download handed to the browser, resolve() an install about to quit, reject() an error.
function updating(os) {
  let resolve, reject;
  const pending = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  const classes = new Set();
  const nodes = Object.fromEntries(['.uw-go', '[data-l]', '[data-t]', '[data-s]'].map((name) => [name, { textContent: '', style: { removeProperty() {} } }]));
  const win = { dataset: {}, classList: { add: (x) => classes.add(x), remove: (x) => classes.delete(x), contains: (x) => classes.has(x) } };
  const errors = [], toasts = [];
  let closed = false;
  const c = vm.createContext({
    S: { os }, invoke: () => pending, $: (selector) => nodes[selector],
    document: { body: { classList: { contains: () => false } }, querySelectorAll: () => [] },
    t: (s) => s, verName: (s) => s, tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, key) => values[key]),
    blocked: () => false, cantReplace: () => false, fail: (error) => errors.push(error),
    lookForLauncher() {}, toast: (msg) => toasts.push(msg),
  });
  vm.runInContext(['selfUpdate', 'updWinGo', 'updWinStart', 'updWinReset'].map(fn).join('\n'), c);
  const done = c.updWinGo(win, { version: 'next' }, () => { closed = true; });
  return { c, nodes, errors, toasts, resolve, reject, done, busy: () => classes.has('busy'), closed: () => closed };
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

test('an install that is quitting the launcher keeps the window busy, so Update Now cannot start another', async () => {
  const c = updating('windows');
  c.resolve(null);
  await c.done;
  assert.equal(c.busy(), true);
  assert.equal(c.closed(), false);
  assert.equal(c.c.S.selfUpdating, true);
  assert.deepEqual(c.toasts, []);
});

test('a download handed to the browser closes the window and says so', async () => {
  const c = updating('macos');
  c.resolve('opened');
  await c.done;
  assert.equal(c.closed(), true);
  assert.equal(c.c.S.selfUpdating, false);
  assert.deepEqual(c.toasts, ['The download page opened in your browser.']);
});

test('the Linux package handoff closes the window too', async () => {
  const c = updating('linux');
  c.resolve('opened');
  await c.done;
  assert.equal(c.closed(), true);
});

test('a failed update resets the window and reports its error', async () => {
  const c = updating('windows');
  const error = new Error('The download failed');
  c.reject(error);
  await c.done;
  assert.equal(c.busy(), false);
  assert.equal(c.closed(), false);
  assert.equal(c.c.S.selfUpdating, false);
  assert.deepEqual(c.errors, [error]);
  assert.equal(c.nodes['[data-l]'].textContent, 'Update Now');
  assert.equal(c.nodes['[data-t]'].textContent, 'Update to next');
});
