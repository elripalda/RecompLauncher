const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

// The copy picker for one game: `setup` is its installed setup, `entry` its catalog entry.
function pickerFor({ setup = null, entry = {} } = {}) {
  const c = vm.createContext({
    S: { setup: setup ? { g: setup } : {} },
    byId: () => ({ id: 'g', ...entry }),
    t: (s) => s,
    // A console with files of its own: used only when the project's file names no types.
    extsFor: (origin) => (origin === 'NES' ? ['nes'] : null),
    originName: (o) => o, tf: (s) => s, builtinBoot: () => ({}), ROM_EXTS: { n64: [], disc: [] },
  });
  vm.runInContext(fn('inputCopyExts') + fn('copyPickOpts'), c);
  return c.copyPickOpts('g');
}

const JAR_INPUT = { id: 'jar', label: 'Your copy', role: 'game', formats: [{ type: 'file', extensions: ['jar'] }] };

test('the catalog manifest\'s named input types the copy picker before install', () => {
  const opts = pickerFor({ entry: { launcher: { inputs: [JAR_INPUT] } } });
  assert.deepEqual(Array.from(opts.filters[0].extensions), ['jar']);
});

test('an installed game\'s setup rows type the copy picker', () => {
  const opts = pickerFor({ setup: { inputs: [{ id: 'jar', role: 'game', active: true, extensions: ['jar'] }] } });
  assert.deepEqual(Array.from(opts.filters[0].extensions), ['jar']);
});

test('the copy picker still offers All files beside the declared types', () => {
  const opts = pickerFor({ entry: { launcher: { inputs: [JAR_INPUT] } } });
  assert.deepEqual(Array.from(opts.filters[1].extensions), ['*']);
});

test('shared keys and other games\' inputs do not type the copy picker', () => {
  const inputs = [
    { id: 'keys', role: 'key', shared: 'switch.prod-keys', active: true, extensions: ['keys'] },
    { id: 'other', role: 'game', active: false, extensions: ['iso'] },
    { id: 'jar', role: 'game', active: true, extensions: ['.JAR'] },
  ];
  assert.deepEqual(Array.from(pickerFor({ setup: { inputs } }).filters[0].extensions), ['jar']);
});

test('without declared types the console\'s own files are used as before', () => {
  const opts = pickerFor({ entry: { origin: 'NES' } });
  assert.deepEqual(Array.from(opts.filters[0].extensions), ['nes']);
});
