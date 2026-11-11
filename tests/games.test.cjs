const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const games = fs.readFileSync(path.join(__dirname, '../ui/games.js'), 'utf8');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

const COG = { id: 'cogbox', name: 'COGBox', cover: '/cog.jpg', hero: '/cogh.jpg', origin: 'x360', title: 'gears', games: [
  { id: 'gow1', name: 'Gears of War', args: ['--game=gow1'], cover: '/g1.jpg' },
  { id: 'gow2', name: 'Gears of War 2', args: ['--game=gow2'], inputs: ['disc2'], saves: ['{install}/gow2'] },
  { id: 'gow3', name: 'Gears of War 3', args: ['--game=gow3'] },
  { id: 'gowj', name: 'Gears of War: Judgment' },
] };

function ctx(extra = {}) {
  const c = vm.createContext(Object.assign({
    S: { library: {}, gameSets: {}, os: 'windows', running: new Set(), settings: { ui: {} } },
    window: {}, invoke: async () => ({}), t: (s) => s, tf: (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => v[k]), tn: (n, one, many) => (n === 1 ? one : many.replace('{n}', n)), esc: (s) => String(s),
  }, extra));
  vm.runInContext(games, c);
  return c;
}
const install = (c) => {
  c.S.library.cogbox = { id: 'cogbox', name: 'COGBox', tag: 'v1', path: '/g/cogbox', size: 900, played_secs: 100, games: { gow2: { played_secs: 60, last_played: 50 } } };
  c.S.library.other = { id: 'other', name: 'Other', tag: 'v2', size: 10 };
  c.S.gameSets = { cogbox: { games: COG.games, ready: { gow1: true, gow2: false, gow3: true, gowj: true } } };
};

test('a list with bad or repeated ids keeps the rest, and one game is none', () => {
  const c = ctx();
  c.list = [{ id: 'a', name: 'A' }, { id: 'a', name: 'Again' }, { id: 'B!', name: 'Bad' }, { id: 'c', name: 'C' }, { id: 'd' }];
  assert.equal(vm.runInContext('gameList(list).map((g) => g.id).join()', c), 'a,c');
  assert.equal(vm.runInContext("gameList([{ id: 'a', name: 'A' }]).length", c), 0);
  assert.equal(vm.runInContext('gameList(undefined).length', c), 0);
});

test('each game is its own app with its own art, falling back to the recomp', () => {
  const c = ctx();
  c.ids = new Map([['cogbox', COG], ['solo', { id: 'solo', name: 'Solo' }]]);
  vm.runInContext('addGameApps(ids)', c);
  assert.equal(c.ids.size, 6);
  const g1 = c.ids.get('cogbox--gow1'), j = c.ids.get('cogbox--gowj');
  assert.equal(g1.name, 'Gears of War');
  assert.equal(g1.cover, '/g1.jpg');
  assert.equal(g1.hero, '/cogh.jpg');
  assert.equal(g1.parent, 'cogbox');
  assert.equal(g1.title, undefined, 'not grouped with the recomp as one of its versions');
  assert.equal(g1.menuPick, false);
  assert.equal(j.menuPick, true, 'no options of its own: picked in the recomp\'s menu');
  assert.equal(vm.runInContext("subOf('cogbox--gow3').rid", c), 'cogbox');
  assert.equal(vm.runInContext("subOf('solo')", c), null);
});

test('one install shows as a tile for each game, with its own play time', () => {
  const c = ctx();
  install(c);
  vm.runInContext('expandLib()', c);
  const recs = vm.runInContext('libRecs().map((g) => g.id).sort().join()', c);
  assert.equal(recs, 'cogbox--gow1,cogbox--gow2,cogbox--gow3,cogbox--gowj,other');
  assert.equal(c.S.library['cogbox--gow2'].played_secs, 60);
  assert.equal(c.S.library['cogbox--gow1'].played_secs, 0);
  assert.equal(c.S.library['cogbox--gow2'].ready, false, 'its own disc isn\'t set up');
  assert.equal(c.S.library['cogbox--gow1'].ready, true);
  // Sizes and installs count the one install.
  assert.equal(vm.runInContext('libInstalls().reduce((n, g) => n + (g.size || 0), 0)', c), 910);
  // Expanding again doesn't pile up, and a recomp's answer stored under a game's id goes back.
  c.S.library['cogbox--gow3'] = Object.assign({}, c.S.library.cogbox, { tag: 'v2' });
  vm.runInContext('expandLib(); expandLib()', c);
  assert.equal(c.S.library.cogbox.tag, 'v2');
  assert.equal(Object.keys(c.S.library).length, 6);
  assert.equal(c.S.library['cogbox--gow3'].tag, 'v2');
});

test('updates and removal go to the one install; playing, setup and achievements go by the game', () => {
  const c = ctx();
  install(c);
  c.ids = new Map([['cogbox', COG]]);
  vm.runInContext('addGameApps(ids); expandLib()', c);
  const ask = (cmd, id) => vm.runInContext(`gameArgs(${JSON.stringify(cmd)}, { id: ${JSON.stringify(id)} }).id`, c);
  for (const cmd of ['install', 'uninstall', 'check_updates', 'open_folder', 'set_game_options', 'stop', 'mods_state']) assert.equal(ask(cmd, 'cogbox--gow1'), 'cogbox', cmd);
  for (const cmd of ['play', 'game_setup', 'setup_inputs_run', 'desktop_shortcut', 'add_to_steam', 'ach_game']) assert.equal(ask(cmd, 'cogbox--gow1'), 'cogbox--gow1', cmd);
  // Saves: the game's own when it declares them, else the recomp's shared ones.
  assert.equal(ask('saves_info', 'cogbox--gow2'), 'cogbox--gow2');
  assert.equal(ask('saves_info', 'cogbox--gow1'), 'cogbox');
  assert.equal(ask('install', 'solo'), 'solo');
  assert.equal(vm.runInContext("gameArgs('get_state', undefined)", c), undefined);
});

test('the tile line and the recomp\'s page: setup, menu pick, part of, and even rows', () => {
  const c = ctx();
  install(c);
  c.ids = new Map([['cogbox', COG]]);
  vm.runInContext('addGameApps(ids); expandLib()', c);
  c.byId = (id) => c.ids.get(id);
  c.tile = (a) => `[${a.id}]`;
  vm.runInContext(fn(app, 'gameTileLine') + fn(app, 'partOfLine') + fn(app, 'includesBox'), c);
  assert.match(vm.runInContext("gameTileLine(byId('cogbox--gow2'))", c), /Set Up/);
  assert.match(vm.runInContext("gameTileLine(byId('cogbox--gowj'))", c), /Pick the game in its menu/);
  assert.match(vm.runInContext("gameTileLine(byId('cogbox--gow1'))", c), /Part of COGBox/);
  assert.match(vm.runInContext("partOfLine(byId('cogbox--gow1'))", c), /data-id="cogbox"/);
  assert.equal(vm.runInContext("partOfLine(byId('cogbox'))", c), '');
  const box = vm.runInContext("includesBox(byId('cogbox'))", c);
  assert.match(box, /Includes 4 Games/);
  assert.match(box, /--inc-cols:4/);
  assert.equal((box.match(/\[cogbox--/g) || []).length, 4);
  // Seven games: two rows of four and three, never one on its own.
  c.ids.set('big', { id: 'big', name: 'Big', games: Array.from({ length: 7 }, (_, i) => ({ id: 'g' + i, name: 'G' + i })) });
  vm.runInContext('addGameApps(ids)', c);
  assert.match(vm.runInContext("includesBox(byId('big'))", c), /--inc-cols:4/);
  assert.equal(vm.runInContext("includesBox(byId('cogbox--gow1'))", c), '');
});

test('Big Picture lists each game on its own and shows the recomp\'s games on its page', () => {
  assert.match(tv, /const installed = \(\) => libRecs\(\)/);
  assert.match(tv, /Includes \{n\} Games/);
  assert.match(tv, /data-go-game="\$\{esc\(a\.parent\)\}"/);
  assert.match(app, /const played = libRecs\(\)/);
  assert.match(app, /let list = libRecs\(\)\.map/);
});
