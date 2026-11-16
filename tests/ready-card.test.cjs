const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// Before you play's copy row, from what game_setup says (copyFormats stands in for the console's formats).
const copyState = (su) => new Function('copyFormats', `${fn(app, 'copyState')}; return copyState;`)(() => ({ exts: ['gb', 'gbc', 'zip'] }))('g', su);

const rom = (path) => ({ id: 'rom-emerald', path, extensions: ['gba', 'zip', '7z'], shared: '', runtime: false });
const keys = { id: 'keys', path: 'C:/keys/prod.keys', extensions: ['keys'], shared: 'switch.prod-keys', runtime: false };

test('a game of several, set up through its own input, shows the file it uses', () => {
  const cs = copyState({ needs: 'x', copy: null, inputs_ready: true, inputs: [rom('C:/roms/emerald.gba')] });
  assert.equal(cs.file, 'C:/roms/emerald.gba');
});

test('a game not set up yet asks for its own input\'s formats, not the console\'s', () => {
  const cs = copyState({ needs: 'x', copy: null, inputs_ready: false, inputs: [rom(null)] });
  assert.equal(cs.file, '');
  assert.deepEqual(cs.exts, ['gba', 'zip', '7z']);
});

test('the install\'s one copy doesn\'t count for a game whose own inputs aren\'t ready', () => {
  const cs = copyState({ needs: 'x', copy: 'C:/roms/red.gb', inputs_ready: false, inputs: [rom(null)] });
  assert.equal(cs.file, '');
});

test('shared keys are never named as the copy nor asked for as one', () => {
  const cs = copyState({ needs: 'x', copy: null, inputs_ready: true, inputs: [keys, rom('C:/roms/emerald.gba')] });
  assert.equal(cs.file, 'C:/roms/emerald.gba');
  assert.deepEqual(cs.exts, ['gba', 'zip', '7z']);
});

test('a game set up with one copy and no named inputs is as before', () => {
  const cs = copyState({ needs: 'x', copy: 'C:/games/game.iso' });
  assert.equal(cs.file, 'C:/games/game.iso');
  assert.deepEqual(cs.exts, ['gb', 'gbc', 'zip']);
});
