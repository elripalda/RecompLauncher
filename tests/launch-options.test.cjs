// Launch options read like Steam's: variables before %command%, arguments after it. Plain text
// without %command% stays arguments, as before.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const dialog = app.match(/^function launchOptions\([^]*?^}/m)[0];

test('the launch options dialog says how to set variables', () => {
  assert.match(dialog, /t\('To set variables for the game too, write them before %command%, like Steam: NAME=value %command% --option\.'\)/);
});

test('saving launch options refuses ones the launcher can\'t read', () => {
  assert.match(lib, /if let Some\(a\) = args \{\s*launch_options::parse\(&a\)\.map_err\(Error::Msg\)\?;/);
});

test('Play sets the player\'s variables after everyone else\'s, so theirs win', () => {
  const play = lib.slice(lib.indexOf('let player = if clean'), lib.indexOf('// Recomp Achievements (test accounts)'));
  assert.ok(play.indexOf('for (k, v) in &game_env') < play.indexOf('for (k, v) in &player.env'));
  assert.match(play, /args\.extend\(player\.args\.iter\(\)\.cloned\(\)\)/);
});

test('a shortcut straight to the game is only made when the launch options have no variables', () => {
  const direct = lib.match(/^fn direct_start\([^]*?^}/m)[0];
  assert.match(direct, /if !player\.env\.is_empty\(\) \{\s*return None;/);
});

test('every language has the new line', () => {
  const dir = path.join(__dirname, '../ui/i18n');
  const key = 'To set variables for the game too, write them before %command%, like Steam: NAME=value %command% --option.';
  for (const f of fs.readdirSync(dir).filter((f) => f.endsWith('.json') && !['en-GB.json', 'fr-CA.json'].includes(f))) {
    const v = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'))[key];
    assert.ok(v && v.includes('%command%') && v.includes('NAME=value'), f);
  }
});

// The core's reasons are fixed texts (consts), so the window's fail() finds them in every language.
test('every reason launch options are refused is translated', () => {
  const rs = fs.readFileSync(path.join(__dirname, '../src-tauri/src/launch_options.rs'), 'utf8');
  const reasons = [...rs.matchAll(/^pub const [A-Z_]+: &str = "([^"]+)";/gm)].map((m) => m[1]).filter((s) => s !== '%command%');
  assert.equal(reasons.length, 5);
  assert.doesNotMatch(rs, /Err\(format!/);
  const dir = path.join(__dirname, '../ui/i18n');
  for (const f of fs.readdirSync(dir).filter((f) => f.endsWith('.json') && !['en-GB.json', 'fr-CA.json'].includes(f))) {
    const lang = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    for (const r of reasons) assert.ok(lang[r], `${f}: ${r}`);
  }
  assert.match(app, /const fail = \(e\) => toast\(t\(String\(e && e\.message \? e\.message : e\)/);
});
