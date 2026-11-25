const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fnOf = (name) => app.match(new RegExp(`^(?:window\\.${name} = )?(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('Start With asks for the programs the game starts, not every program in its folder', () => {
  assert.match(fnOf('loadStartWith'), /invoke\('start_with_programs', \{ id \}\)/);
  assert.doesNotMatch(fnOf('loadStartWith'), /game_programs/);
});

test('picking the program for a game the launcher can\'t place still sees every program', () => {
  assert.match(fnOf('askProgram'), /invoke\('game_programs', \{ id \}\)/);
});

test('the core has the Start With command', () => {
  const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
  assert.match(lib, /game_programs, start_with_programs, run_program/);
});

test('a game Easy Setup runs has no Start With', () => {
  assert.match(app, /const startWithOffered = \(id\) => !!S\.library\[id\] && !S\.easy\.has\(id\);/);
  assert.match(fnOf('loadStartWith'), /if \(!startWithOffered\(id\)\) return \[\];/);
});

test('Start With needs two or more programs to pick from', () => {
  assert.match(app, /const startWithList = \(id\) => \(startWithOffered\(id\) && !S\.running\.has\(id\) && \(START_WITH\[id\] \|\| \[\]\)\.length > 1/);
});

test('the "…" menu has Start With at its top, as a group of the programs', () => {
  assert.match(fnOf('gameMenu'), /return \[\n\s+\.\.\.startWithItem\(id\),/);
  assert.match(fnOf('startWithItem'), /\{ label: t\('Start With'\), icon: 'play', sub: rows \}/);
  assert.match(app, /case 'start-with': return playGame\(id, \(START_WITH\[id\] \|\| \[\]\)\[Number\(el\.dataset\.k\)\]\);/);
});

test('Start With\'s rows show each program\'s path as it is, not in Title Case', () => {
  assert.match(fnOf('startWithItem'), /raw: true/);
  assert.match(fnOf('menuRows'), /const raw = i\.raw \? ' data-raw' : '';/);
  const tc = fs.readFileSync(path.join(__dirname, '../ui/titlecase.js'), 'utf8');
  assert.match(tc, /\[data-raw\]/);
});

test('the "…" menu loads the programs before it opens', () => {
  assert.match(app, /case 'menu': return openGameMenu\(el, id\);/);
  assert.match(fnOf('openGameMenu'), /await loadStartWith\(id\);\n\s+return menu\(at, gameMenu\(id\)\);/);
});

test('holding Play no longer opens Start With', () => {
  assert.doesNotMatch(app, /data-held/);
});

test('Big Picture has Start With at the top of the Options sheet, by the same rule', () => {
  assert.match(tv, /\.\.\.setupItem,\n.*\n.*\n\s+\.\.\.\(startWithList\(id\)\.length \? \[\{ label: t\('Start With'\)/);
  assert.match(tv, /case 'more': sfx\('open'\); return loadStartWith\(id\)\.then\(\(\) => gameSheet\(id\)\);/);
});
