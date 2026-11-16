const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const rust = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// "Program it starts" on the game's page, with what game_setup says about the game.
const row = (setup, easy) => {
  const run = new Function('S', 't', 'esc', `${fn(app, 'setupOf')}; ${fn(app, 'programRow')}; return programRow;`);
  const S = { setup: { g: setup }, easyAll: new Set(easy ? ['g'] : []) };
  return run(S, (x) => x, (x) => x)('g', { exe: 'Game\\bin\\game.exe' });
};

test('a game Easy Setup does says what picking its program does', () => {
  const html = row({ own_program: null }, true);
  assert.match(html, /Pick its own program, and Play just starts that, without Easy Setup\./);
  assert.doesNotMatch(html, /forget-program/);
});

test('a game Easy Setup doesn\'t do shows only its program', () => {
  const html = row(null, false);
  assert.match(html, /Game\\bin\\game\.exe/);
  assert.doesNotMatch(html, /Easy Setup/);
});

test('once the player\'s program has taken over, the row says so and offers the way back', () => {
  const html = row({ own_program: 'D:\\Game\\bin\\game.exe' }, true);
  assert.match(html, /Play starts this program as it is\. Easy Setup and the game's own way of starting it are skipped\./);
  assert.match(html, /data-act="forget-program"/);
});

test('the Manage menu marks Choose Program as on', () => {
  assert.match(fn(app, 'gameMenu'), /act: 'setexe', id, label: setupOf\(id\) && setupOf\(id\)\.own_program \? t\('Choose Program \(On\)…'\) : t\('Choose Program…'\)/);
});

test('Point to Your Build offers the game\'s program too, which Play then starts', () => {
  assert.match(app, /\{ label: t\('Choose Folder'\), v: 'pick', main: true \}, \{ label: t\('Choose Program'\), v: 'program' \}, \{ label: t\('Use Easy Setup'\), v: 'easy' \}/);
  assert.match(app, /if \(v === 'program'\) \{\n\s+try \{ if \(!\(await chooseProgram\(id\)\)\) return; \} catch \(e\) \{ return fail\(e\); \}\n\s+return playGame\(id\);/);
});

test('the way back lets the pick go in the core', () => {
  assert.match(app, /case 'forget-program':[^]*?invoke\('forget_program', \{ id \}\)/);
  assert.match(rust, /set_exe, forget_program, open_folder,/);
  assert.match(rust, /own_program: player_program\(&st, &id, &g\),/);
  // Only the player's pick is let go: a program the launcher picked stays.
  assert.match(fn(rust.replace(/^fn /gm, 'function '), 'forget_program'), /if g\.exe_rules == u8::MAX \{\n\s+g\.exe = None;\n\s+g\.exe_rules = 0;/);
});
