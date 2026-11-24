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

test('a game Easy Setup runs has no Start With: its own program is the one that plays', () => {
  assert.match(app, /const startWithOffered = \(id\) => !!S\.library\[id\] && !S\.easy\.has\(id\);/);
  assert.match(fnOf('startWith'), /if \(!startWithOffered\(id\)\) return playGame\(id\);/);
});

test('holding Play only opens Start With for a game that offers it', () => {
  assert.match(app, /!b \|\| e\.button !== 0 \|\| !startWithOffered\(b\.dataset\.id\)/);
});

test('Big Picture offers Start With by the same rule as the desktop', () => {
  const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
  assert.match(tv, /startWithOffered\(id\) && !S\.running\.has\(id\) \? \[\{ label: t\('Start With'\)/);
});
