const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// Before you play's copy row, from what game_setup says (copyFormats stands in for the console's formats).
const copyState = (su) => new Function('copyFormats', `${fn(app, 'copyState')}; return copyState;`)(() => ({ exts: ['gb', 'gbc', 'zip'] }))('g', su);
// Its other files' rows.
const otherFileRows = (su) => new Function('tf', 'baseName', 'esc', 't', `${fn(app, 'otherFileRows')}; return otherFileRows;`)((s, o) => s.replace('{file}', o.file), (p) => p.split('/').pop(), (x) => x, (x) => x)('g', su);

const rom = ['gba', 'zip', '7z'];
const keys = { id: 'keys', label: 'Your prod.keys', path: 'C:/keys/prod.keys', active: true, optional: false, shared: 'switch.prod-keys', runtime: false };

test('a game of several, set up through its own input, shows the file it uses', () => {
  // game_setup sends the game's main input as the copy, with its formats.
  const cs = copyState({ needs: 'x', copy: 'C:/roms/emerald.gba', copy_extensions: rom, inputs_ready: true, inputs: [] });
  assert.equal(cs.file, 'C:/roms/emerald.gba');
});

test('a game not set up yet asks for its own input\'s formats, not the console\'s', () => {
  const cs = copyState({ needs: 'x', copy: null, copy_extensions: rom, inputs_ready: false, inputs: [] });
  assert.equal(cs.file, '');
  assert.deepEqual(cs.exts, rom);
});

test('a copy set up before, with a new build due after an update, is still shown as set up', () => {
  const cs = copyState({ needs: 'x', copy: 'C:/games/sky', copy_extensions: [], inputs_ready: false, status: { ready: false, reason: 'rebuild', changed: true } });
  assert.equal(cs.file, 'C:/games/sky');
});

test('a copy picked and not set up yet isn\'t shown as set up', () => {
  const cs = copyState({ needs: 'x', copy: 'C:/roms/red.gb', copy_extensions: rom, inputs_ready: false, inputs: [] });
  assert.equal(cs.file, '');
});

test('shared keys are never named as the copy: they\'re a row of their own', () => {
  const su = { needs: 'x', copy: 'C:/roms/emerald.gba', copy_extensions: rom, inputs_ready: true, inputs: [keys] };
  assert.equal(copyState(su).file, 'C:/roms/emerald.gba');
  const rows = otherFileRows(su);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].k, 'Your prod.keys');
  assert.equal(rows[0].v, true);
});

test('a copy with no formats of its own asks for the console\'s', () => {
  const cs = copyState({ needs: 'x', copy: 'C:/games/game.iso', inputs_ready: true });
  assert.equal(cs.file, 'C:/games/game.iso');
  assert.deepEqual(cs.exts, ['gb', 'gbc', 'zip']);
});

test('the update is a row with the game\'s other files, asked for until it\'s picked', () => {
  const update = { id: 'update', label: 'Your 1.02 update', path: null, active: true, optional: false, shared: '', runtime: false };
  const dlc = { id: 'dlc', label: 'DLC', path: null, active: true, optional: true, shared: '', runtime: false };
  const rows = otherFileRows({ needs: 'x', inputs: [update, dlc] });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].k, 'Your 1.02 update');
  assert.equal(rows[0].v, null);
  assert.match(rows[0].fix, /data-act="setup"/);
});
