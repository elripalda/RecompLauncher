const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fnOf = (name) => app.match(new RegExp(`^(?:window\\.${name} = )?(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('Start With asks for the programs the game starts, not every program in its folder', () => {
  assert.match(fnOf('startWith'), /invoke\('start_with_programs', \{ id \}\)/);
  assert.doesNotMatch(fnOf('startWith'), /game_programs/);
});

test('picking the program for a game the launcher can\'t place still sees every program', () => {
  assert.match(fnOf('askProgram'), /invoke\('game_programs', \{ id \}\)/);
});

test('the core has the Start With command', () => {
  const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
  assert.match(lib, /game_programs, start_with_programs, run_program/);
});
