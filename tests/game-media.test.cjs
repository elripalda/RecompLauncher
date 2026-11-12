const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), cz = read('customize.js');

// The picture rules out of app.js (gameKey, catalogArt, usualArt, applyCover), run against a
// small catalog: a racing game with two versions, "race-rec" the recommended one.
function load(ui, arts = {}, library = {}) {
  const start = app.indexOf('// The pictures Customize changes');
  const end = app.indexOf('const byId = ');
  assert.ok(start >= 0 && end > start);
  const rec = { id: 'race-rec', title: 'race', name: 'Racer Recomp', cover: 'rec-cover', icon: 'rec-icon' };
  const neo = { id: 'race-neo', title: 'race', name: 'Racer Neo', cover: 'neo-cover', icon: 'neo-icon' };
  const ids = new Map([[rec.id, rec], [neo.id, neo]]);
  const group = { id: 'race', pick: 'race-rec', list: [rec, neo], every: [rec, neo] };
  const S = { settings: { ui }, arts, library };
  const verGroup = (a) => (a && a.title === 'race' ? group : null);
  const index = () => ({ ids });
  const f = new Function('S', 'verGroup', 'index', `${app.slice(start, end)} return { applyCover, catalogArt };`);
  return Object.assign(f(S, verGroup, index), { rec, neo, S });
}

test('a version shows its own store pictures when Show Game Media is off', () => {
  const x = load({});
  assert.equal(x.applyCover(x.neo, 'race-neo').cover, 'neo-cover');
});

test('Show Game Media gives every version the recommended version\'s pictures', () => {
  const x = load({ game_media: true });
  assert.equal(x.applyCover(x.neo, 'race-neo').cover, 'rec-cover');
  assert.equal(x.neo.icon, 'rec-icon');
});

test('the versions list still finds each version\'s own icon under Show Game Media', () => {
  const x = load({ game_media: true });
  x.applyCover(x.neo, 'race-neo');
  assert.equal(x.catalogArt(x.neo, 'icon'), 'neo-icon');
});

test('turning Show Game Media off brings a version\'s own pictures back', () => {
  const x = load({ game_media: true });
  x.applyCover(x.neo, 'race-neo');
  x.S.settings.ui = {};
  assert.equal(x.applyCover(x.neo, 'race-neo').cover, 'neo-cover');
});

test('a picture the player set for one version wins over Show Game Media', () => {
  const x = load({ game_media: true }, { 'race-neo': { cover_file: 'C:/c/neo.jpg' } });
  assert.equal(x.applyCover(x.neo, 'race-neo').cover, 'local:C:/c/neo.jpg');
});

test('a picture set for every version shows on a version that has none of its own', () => {
  const x = load({}, { 'game--race': { cover_file: 'C:/c/game.jpg' } });
  assert.equal(x.applyCover(x.neo, 'race-neo').cover, 'local:C:/c/game.jpg');
});

test('a name set for every version shows on a version without its own', () => {
  const x = load({ names: { 'game--race': 'Racer' } });
  assert.equal(x.applyCover(x.neo, 'race-neo').name, 'Racer');
});

test('the versions menu and card draw each version\'s own icon', () => {
  assert.match(app, /const verIcon = \(v\) => catalogArt\(v, 'icon'\) \|\| catalogArt\(v, 'cover'\);/);
  assert.match(app, /const ic = verIcon\(v\);/);
});

test('Customize offers its every-version box only for a game with versions', () => {
  assert.match(cz, /\$\{group \? `<label class="cz-every">/);
});
