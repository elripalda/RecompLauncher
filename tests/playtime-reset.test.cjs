// Reset Play Time (issue #487): a game's menu and its Big Picture sheet offer it once this PC has
// counted time for the game.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// hasOwnPlaytime with this PC's own record for the game.
const hasOwn = (play) => new Function('localPlaytime', `${fn(app, 'hasOwnPlaytime')}; return hasOwnPlaytime;`)(() => play)('g');

test('a game played on this PC has play time to reset', () => {
  assert.equal(hasOwn({ played_secs: 120 }), true);
});

test('a game never played on this PC has none', () => {
  assert.equal(hasOwn({ played_secs: 0 }), false);
});

test('a game with no record on this PC has none', () => {
  assert.equal(hasOwn(undefined), false);
});

test('the game menu offers Reset Play Time when there is time to reset', () => {
  assert.match(fn(app, 'gameMenu'), /hasOwnPlaytime\(id\) \? \[\{ act: 'play-reset', id, label: t\('Reset Play Time…'\)/);
});

test('the menu item runs the reset', () => {
  assert.match(app, /case 'play-reset': return resetPlaytime\(id\);/);
});

test('Big Picture offers it too, not while the game runs', () => {
  assert.match(tv, /!S\.running\.has\(id\) && window\.hasOwnPlaytime\(id\) \? \[\{ label: t\('Reset Play Time'\)/);
});

test('the reset asks first and keeps the game by default', () => {
  const reset = fn(app, 'resetPlaytime');
  assert.match(reset, /await ask\(/);
  assert.match(reset, /if \(v !== 'yes'\) return;/);
  assert.match(reset, /\{ label: t\('Keep it'\), sub: t\('Nothing changes'\), v: 'no', focus: true \}/);
});

test('an account that could not be cleared is said, not hidden', () => {
  assert.match(fn(app, 'resetPlaytime'), /if \(r\.account === false\) return toast\(/);
});
