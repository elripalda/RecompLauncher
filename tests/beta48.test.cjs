const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const named = read('setup-inputs.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('a profile input named "game" is set like any other input, not as the old single copy', async () => {
  const calls = [];
  const c = vm.createContext({
    S: { setup: { hop: { inputs: [{ id: 'game', label: 'Your game', role: 'game', active: true, file: true, folder: true, extensions: ['nsp'] }] } } },
    pickFile: async () => '/games/hopper.nsp',
    invoke: async (cmd, args) => { calls.push([cmd, args && args.input]); return cmd === 'game_setup' ? { inputs: [] } : null; },
    refreshState: async () => {},
  });
  vm.runInContext(fn(named, 'namedInputRows') + fn(named, 'namedInputsAction'), c);
  await c.namedInputsAction('hop', 'file', 'game', {});
  assert.deepEqual(calls[0], ['setup_input_set', 'game']);
  // The old single-copy profile still goes to its own command.
  calls.length = 0;
  c.S.setup.old = { kind: 'file', needs: 'Your copy', inputs: [] };
  await c.namedInputsAction('old', 'file', 'game', {});
  assert.deepEqual(calls[0], ['set_game_copy', 'game']);
});

test('a picked profile input named "game" is changed by choosing again, with the same buttons', () => {
  // The steps never take a needed file's buttons away (choosing again changes it); optional ones
  // and shared keys can be removed, with their × kept in place while empty.
  const steps = read('setup-steps.js');
  assert.match(fn(steps, 'ssRow'), /const canRemove = r\.optional \|\| \(r\.shared && !r\.legacy\);/);
  assert.match(fn(steps, 'ssRow'), /<div class="acts">\$\{ssPickBtns\(r, ns\.busy\)\}\$\{rm\}<\/div>/);
});

test("a pointed-at cover isn't cut off where the clip can't be widened", () => {
  const css = read('app.css');
  assert.match(css, /@supports not \(overflow-clip-margin:24px\)\{[^}]*\.shelf \.track>\.tile:is\(:hover,:focus-within,\.on\)\{content-visibility:visible\}/);
});

test('a setup finished in the background says so only when its window is closed', () => {
  const app = read('app.js');
  const src = fn(app, 'onSetupFinished');
  assert.match(src, /if \(!watching\) toast\(tf\('\{name\} is ready to play!'/);
  // (The update is one of its files now, picked with the rest: nothing is left to ask after.)
  assert.doesNotMatch(src, /su\.update\b/);
});
