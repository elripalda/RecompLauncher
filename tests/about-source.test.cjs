const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js');

// aboutSource out of app.js: who the line under About the game credits.
function load() {
  const start = app.indexOf('function aboutSource(ab) {');
  const end = app.indexOf('// Facts about the original game');
  assert.ok(start >= 0 && end > start);
  return new Function('t', `${app.slice(start, end)} return aboutSource;`)((s) => s);
}

test('info from LaunchBox credits LaunchBox, not IGDB', () => {
  const s = load()({ src: 'launchbox', launchbox: 1, name: 'Star Fox 64' });
  assert.equal(s.label, 'Game info from LaunchBox Games Database');
  assert.equal(s.url, 'https://gamesdb.launchbox-app.com/');
});

test('info from IGDB still links its IGDB page', () => {
  const s = load()({ igdb: 5, url: 'https://www.igdb.com/games/star-fox-64' });
  assert.equal(s.label, 'Game info from IGDB');
  assert.equal(s.url, 'https://www.igdb.com/games/star-fox-64');
});

test('no source and no secure link credits nobody', () => {
  assert.equal(load()({ name: 'x', url: 'javascript:alert(1)' }), null);
});

test('Big Picture uses the same credit', () => {
  assert.match(tv, /aboutSource\(ab\)/);
  assert.doesNotMatch(tv, /t\('Game info from IGDB'\)/);
});
