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
  // A recomp with both sets: the game's pictures and its project's own look.
  const zel = { id: 'zelda', name: 'Zelda Recomp', game: 'Zelda', cover: 'game-cover', hero: 'game-hero', icon: 'game-icon', project_cover: 'proj-cover', project_icon: 'proj-icon', developer_avatar: 'dev-face' };
  const ids = new Map([[rec.id, rec], [neo.id, neo], [zel.id, zel]]);
  const group = { id: 'race', pick: 'race-rec', list: [rec, neo], every: [rec, neo] };
  const S = { settings: { ui }, arts, library };
  const verGroup = (a) => (a && a.title === 'race' ? group : null);
  const index = () => ({ ids });
  const f = new Function('S', 'verGroup', 'index', `${app.slice(start, end)} return { applyCover, catalogArt, pickArt };`);
  return Object.assign(f(S, verGroup, index), { rec, neo, zel, S });
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

test('a recomp shows its project\'s pictures first', () => {
  const x = load({});
  x.applyCover(x.zel, 'zelda');
  assert.equal(x.zel.cover, 'proj-cover');
  assert.equal(x.zel.icon, 'proj-icon');
});

test('a picture the project lacks is the game\'s', () => {
  const x = load({});
  assert.equal(x.applyCover(x.zel, 'zelda').hero, 'game-hero');
});

test('a catalog without project pictures shows the game\'s as before', () => {
  const x = load({});
  assert.equal(x.applyCover(x.rec, 'race-rec').cover, 'rec-cover');
});

test('Show Game Names shows the game\'s pictures only', () => {
  const x = load({ game_names: true });
  x.applyCover(x.zel, 'zelda');
  assert.equal(x.zel.cover, 'game-cover');
  assert.equal(x.zel.icon, 'game-icon');
});

test('turning Show Game Names off brings the project\'s pictures back', () => {
  const x = load({ game_names: true });
  x.applyCover(x.zel, 'zelda');
  x.S.settings.ui = {};
  assert.equal(x.applyCover(x.zel, 'zelda').cover, 'proj-cover');
});

test('the game\'s picture stays the field\'s own while the project\'s shows over it', () => {
  const x = load({});
  x.applyCover(x.zel, 'zelda');
  assert.equal(x.pickArt(x.zel, 'cover', true), 'game-cover');
});

test('a picture the player set wins over both sets', () => {
  const x = load({}, {}, { zelda: { cover_file: 'C:/c/mine.jpg' } });
  assert.equal(x.applyCover(x.zel, 'zelda').cover, 'local:C:/c/mine.jpg');
  x.S.settings.ui = { game_names: true };
  assert.equal(x.applyCover(x.zel, 'zelda').cover, 'local:C:/c/mine.jpg');
});

test('the developer\'s picture is never a recomp\'s icon', () => {
  const x = load({});
  const a = { icon: 'dev-face', project_icon: 'dev-face', cover: 'c', developer_avatar: 'dev-face' };
  assert.equal(x.pickArt(a, 'icon', false), '');
  assert.equal(x.pickArt(a, 'icon', true), '');
});

test('the versions list icon is the project\'s too', () => {
  const x = load({});
  assert.equal(x.catalogArt(x.zel, 'icon'), 'proj-icon');
});

test('the developer\'s picture shows on their line, desktop and Big Picture', () => {
  const src = app.match(/^const devPic = .*$/m)[0];
  const devPic = new Function('devOf', `${src} return devPic;`)(() => ({ avatar: 'page-face' }));
  assert.equal(devPic({ developer_avatar: 'dev-face' }), 'dev-face');
  assert.equal(devPic({}), 'page-face');
  assert.match(app, /const face = devPic\(a\);/);
  assert.match(read('tv.js'), /const face = devPic\(a\);/);
});

test('Show Game Names refreshes the pictures of shortcuts already made', () => {
  assert.match(app, /sw\('game_names', ui\.game_names, t\('Show Game Names'\), 'game-names'\)/);
  assert.match(app, /case 'game-names': return saveUi\(\{ game_names: !\(S\.settings\.ui \|\| \{\}\)\.game_names \}\)\.then\(refreshShortcutArt\);/);
  assert.match(app, /invoke\('refresh_shortcut_art'\)/);
});

test('the versions menu and card draw each version\'s own icon', () => {
  assert.match(app, /const verIcon = \(v\) => catalogArt\(v, 'icon'\) \|\| catalogArt\(v, 'cover'\);/);
  assert.match(app, /const ic = verIcon\(v\);/);
});

test('Customize offers its every-version box only for a game with versions', () => {
  assert.match(cz, /\$\{group \? `<label class="cz-every">/);
});
