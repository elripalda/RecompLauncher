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
    originName: (o) => o, tf: (s) => s, ROM_EXTS: { n64: [], disc: [] },
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

test('with declared types the copy picker offers only those, no All files', () => {
  const opts = pickerFor({ entry: { launcher: { inputs: [JAR_INPUT] } } });
  assert.equal(opts.filters.length, 1);
});

test('a game block\'s copy is not typed by a named input of role game (installed)', () => {
  const setup = { kind: 'iso', inputs: [{ id: 'second', role: 'game', active: true, file: true, extensions: ['bin'] }] };
  assert.deepEqual(Array.from(pickerFor({ setup }).filters[0].extensions), ['*']);
});

test('a game block\'s copy is not typed by a named input of role game (catalog)', () => {
  const opts = pickerFor({ entry: { launcher: { game: { kind: 'iso' }, inputs: [JAR_INPUT] } } });
  assert.deepEqual(Array.from(opts.filters[0].extensions), ['*']);
});

test('a copy input that takes any file leaves the picker open to any file', () => {
  const inputs = [
    { id: 'jar', role: 'game', active: true, file: true, extensions: ['jar'] },
    { id: 'data', role: 'disc', active: true, file: true, extensions: [] },
  ];
  assert.deepEqual(Array.from(pickerFor({ setup: { inputs } }).filters[0].extensions), ['*']);
});

test('shared keys and other games\' inputs do not type the copy picker', () => {
  const inputs = [
    { id: 'keys', role: 'key', shared: 'switch.prod-keys', active: true, extensions: ['keys'] },
    { id: 'other', role: 'game', active: false, extensions: ['iso'] },
    { id: 'jar', role: 'game', active: true, extensions: ['.JAR'] },
  ];
  assert.deepEqual(Array.from(pickerFor({ setup: { inputs } }).filters[0].extensions), ['jar']);
});

test('an optional disc input does not type the copy picker', () => {
  const inputs = [
    { id: 'jar', role: 'game', active: true, extensions: ['jar'] },
    { id: 'bonus', role: 'disc', optional: true, active: true, extensions: ['iso'] },
  ];
  assert.deepEqual(Array.from(pickerFor({ setup: { inputs } }).filters[0].extensions), ['jar']);
});

test('without declared types the console\'s own files are used as before', () => {
  const opts = pickerFor({ entry: { origin: 'NES' } });
  assert.deepEqual(Array.from(opts.filters[0].extensions), ['nes']);
});
