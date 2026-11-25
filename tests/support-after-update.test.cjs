// #296: the Launcher Support card reads the installed release's own recomplauncher.json again
// whenever the installed files change, not only after an install from this launcher.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

// Each support_local call waits until the test answers it.
function context() {
  const asked = [];
  const c = vm.createContext({
    S: { library: {}, supLocal: {}, view: 'library' },
    invoke: (cmd, args) => new Promise((resolve) => asked.push({ cmd, args, resolve })),
    refreshGame: () => {},
  });
  vm.runInContext(app.match(/^const supLocalKey = .*$/m)[0].replace(/^const /, 'globalThis.'), c);
  vm.runInContext(fn('supLocalFor'), c);
  return { c, asked };
}

const flush = () => new Promise((r) => setImmediate(r));
const own = { easy_setup: true };

test('a changed tag asks support_local again', async () => {
  const { c, asked } = context();
  c.S.library.zelda = { path: '/games/zelda', tag: 'v1.0.0', updated_at: 1 };
  c.supLocalFor('zelda');
  asked[0].resolve(null);
  await flush();
  c.S.library.zelda = { path: '/games/zelda', tag: 'v1.1.0', updated_at: 1 };
  c.supLocalFor('zelda');
  assert.equal(asked.length, 2);
  assert.equal(asked[1].cmd, 'support_local');
});

test('an unchanged install does not ask support_local again', async () => {
  const { c, asked } = context();
  c.S.library.zelda = { path: '/games/zelda', tag: 'v1.0.0', updated_at: 1 };
  c.supLocalFor('zelda');
  asked[0].resolve(own);
  await flush();
  const local = c.supLocalFor('zelda');
  assert.equal(asked.length, 1);
  assert.equal(local, own);
});

test('a late answer for files that have since changed is dropped', async () => {
  const { c, asked } = context();
  c.S.library.zelda = { path: '/games/zelda', tag: 'v1.0.0', updated_at: 1 };
  c.supLocalFor('zelda');
  c.S.library.zelda = { path: '/games/zelda', tag: 'v1.1.0', updated_at: 2 };
  c.supLocalFor('zelda');
  asked[0].resolve(own);
  await flush();
  assert.equal(c.supLocalFor('zelda'), null);
  assert.equal(asked.length, 2);
});
