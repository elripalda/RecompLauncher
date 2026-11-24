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
  // The game's own picture of a kind, as app.js reads it (whatever shows over it).
  vm.runInContext(`${app.match(/^const ART_KINDS = .*$/m)[0]}\n${fn(app, 'gameArt')}`, c);
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

test('a game with its own cover doesn\'t take the recomp\'s project cover', () => {
  const c = ctx();
  c.ids = new Map([['cogbox', Object.assign({}, COG, { project_cover: '/cog-p.jpg', project_hero: '/cogh-p.jpg' })]]);
  vm.runInContext('addGameApps(ids)', c);
  const g1 = c.ids.get('cogbox--gow1'), g3 = c.ids.get('cogbox--gow3');
  assert.equal(g1.cover, '/g1.jpg');
  assert.equal(g1.project_cover, '', 'every game of the series would show the same cover');
  assert.equal(g1.project_hero, '/cogh-p.jpg', 'no hero of its own: the recomp\'s look');
  assert.equal(g3.project_cover, '/cog-p.jpg', 'no cover of its own: the recomp\'s look');
});

test('a game\'s own project picture comes with it', () => {
  const c = ctx();
  const box = Object.assign({}, COG, { games: COG.games.map((g) => (g.id === 'gow1' ? Object.assign({}, g, { project_cover: '/g1-p.jpg' }) : g)) });
  c.ids = new Map([['cogbox', box]]);
  vm.runInContext('addGameApps(ids)', c);
  assert.equal(c.ids.get('cogbox--gow1').project_cover, '/g1-p.jpg');
});

test('a game takes the recomp\'s game picture, not one the player set for the recomp', () => {
  const c = ctx();
  c.ids = new Map([['cogbox', Object.assign({}, COG, { hero: 'local:C:/c/mine.jpg', _hero: '/cogh.jpg' })]]);
  vm.runInContext('addGameApps(ids)', c);
  assert.equal(c.ids.get('cogbox--gow1').hero, '/cogh.jpg');
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

test('original game playtime includes removed recomps and keeps each version visible', () => {
  const first = { id: 'hookshot', title: 'ocarina' };
  const second = { id: 'longshot', title: 'ocarina' };
  const bundle = { ...COG, title: 'ocarina' };
  const c = ctx({
    first, second, bundle,
    index: () => ({ titles: new Map([['ocarina', { every: [first, second, bundle] }]]) }),
    playedFor: (play) => play?.played_secs || 0,
  });
  install(c);
  c.S.library.hookshot = { played_secs: 3600, last_played: 100 };
  c.S.removedPlayHistory = { longshot: { played_secs: 3600, last_played: 200 } };
  assert.equal(vm.runInContext('gamePlaytime(first).played_secs', c), 7200);
  assert.equal(vm.runInContext('gamePlaytime(second).last_played', c), 200);
  assert.equal(vm.runInContext('gamePlaytime(bundle).played_secs', c), 100);
  assert.equal(vm.runInContext('JSON.stringify(playtimeRows(second, gamePlaytime(second)))', c),
    JSON.stringify([['Total game playtime', 7200], ['This recomp', 3600]]));
  c.S.library.longshot = { played_secs: 3900, last_played: 300 };
  assert.equal(vm.runInContext('gamePlaytime(first).played_secs', c), 7500);
});

function ocarina() {
  const first = { id: 'hookshot', title: 'ocarina' };
  const second = { id: 'longshot', title: 'ocarina' };
  const c = ctx({
    first, second,
    index: () => ({ titles: new Map([['ocarina', { every: [first, second] }]]) }),
    playedFor: (play) => play?.played_secs || 0,
  });
  c.S.library.hookshot = { played_secs: 600, last_played: 100 };
  return c;
}

test('signed in, the original game shows the account total over all its PCs', () => {
  const c = ocarina();
  c.S.cloudPlay = { games: { hookshot: { secs: 900, last: 300 } }, titles: { ocarina: { secs: 1500, last: 300 } } };
  assert.equal(vm.runInContext('gamePlaytime(first).played_secs', c), 1500);
  assert.equal(vm.runInContext('gamePlaytime(first).last_played', c), 300);
});

test('signed in, a recomp shows its own total over all PCs', () => {
  const c = ocarina();
  c.S.cloudPlay = { games: { hookshot: { secs: 900, last: 300 } }, titles: {} };
  assert.equal(vm.runInContext('recompPlaytime("hookshot").played_secs', c), 900);
});

test('a session the account has not heard about yet still counts', () => {
  const c = ocarina();
  c.S.cloudPlay = { games: { hookshot: { secs: 300, last: 50 } }, titles: { ocarina: { secs: 300, last: 50 } } };
  assert.equal(vm.runInContext('gamePlaytime(first).played_secs', c), 600);
  assert.equal(vm.runInContext('recompPlaytime("hookshot").last_played', c), 100);
});

test('signed out, playtime is this PC\'s own', () => {
  const c = ocarina();
  c.S.cloudPlay = null;
  assert.equal(vm.runInContext('gamePlaytime(first).played_secs', c), 600);
});

// A session start this many days ago.
const daysAgo = (days) => Math.floor(Date.now() / 1000) - days * 86400;
const stats = (c, own) => {
  c.own = own;
  return vm.runInContext('playStats("hookshot", own)', c);
};

test('signed in, the play time card has every PC\'s recent sessions, each once', () => {
  const c = ctx();
  const mine = [daysAgo(2), 60], other = [daysAgo(1), 90];
  c.S.cloudPlay = { games: { hookshot: { count: 2, longest: 90, recent: [mine, other] } }, titles: {} };
  assert.deepEqual(JSON.parse(JSON.stringify(stats(c, [mine]).recent)), [mine, other]);
});

test('signed in, sessions and longest are the account\'s, older sessions included', () => {
  const c = ctx();
  c.S.cloudPlay = { games: { hookshot: { count: 40, longest: 7200, recent: [[daysAgo(1), 90]] } }, titles: {} };
  const s = stats(c, [[daysAgo(1), 90]]);
  assert.equal(s.count, 40);
  assert.equal(s.longest, 7200);
});

test('a session the account has not heard about yet is still on the card', () => {
  const c = ctx();
  const old = [daysAgo(3), 60], fresh = [daysAgo(0), 3000];
  c.S.cloudPlay = { games: { hookshot: { count: 1, longest: 60, recent: [old] } }, titles: {} };
  const s = stats(c, [old, fresh]);
  assert.equal(s.count, 2);
  assert.equal(s.longest, 3000);
  assert.equal(s.recent.length, 2);
});

test('signed out, the card is this PC\'s own: every session counted, the last month drawn', () => {
  const c = ctx();
  c.S.cloudPlay = null;
  const s = stats(c, [[daysAgo(90), 5000], [daysAgo(1), 60]]);
  assert.equal(s.count, 2);
  assert.equal(s.longest, 5000);
  assert.equal(s.recent.length, 1);
});

test('bad sessions and numbers from the website are left off the card', () => {
  const c = ctx();
  const good = [daysAgo(1), 60];
  c.S.cloudPlay = { games: { hookshot: { count: 'lots', longest: -5, recent: [good, ['x', 5], [50], [0, 30], [daysAgo(1), -1], 'no'] } }, titles: {} };
  const s = stats(c, []);
  assert.deepEqual(JSON.parse(JSON.stringify(s)), { recent: [good], count: 1, longest: 60 });
});

test('a game played only on another PC shows the account time', () => {
  const c = ocarina();
  c.S.cloudPlay = { games: { longshot: { secs: 120, last: 400 } }, titles: {} };
  assert.equal(vm.runInContext('recompPlaytime("longshot").played_secs', c), 120);
});

test('updates and removal go to the one install; playing, setup and achievements go by the game', () => {
  const c = ctx();
  install(c);
  c.ids = new Map([['cogbox', COG]]);
  vm.runInContext('addGameApps(ids); expandLib()', c);
  const ask = (cmd, id) => vm.runInContext(`gameArgs(${JSON.stringify(cmd)}, { id: ${JSON.stringify(id)} }).id`, c);
  for (const cmd of ['install', 'uninstall', 'check_updates', 'open_folder', 'set_game_options', 'stop', 'mods_state']) assert.equal(ask(cmd, 'cogbox--gow1'), 'cogbox', cmd);
  for (const cmd of ['play', 'game_info', 'game_setup', 'setup_copy', 'setup_inputs_run', 'setup_cancel','desktop_shortcut', 'add_to_steam', 'ach_game']) assert.equal(ask(cmd, 'cogbox--gow1'), 'cogbox--gow1', cmd);
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
  c.LAYERS = '';
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

// Discover's catalog: COGBox with its four games, and a recomp of one game.
function discoverCtx(hidden = []) {
  const c = ctx({ isHidden: (id) => hidden.includes(id) });
  c.ids = new Map([['cogbox', COG], ['solo', { id: 'solo', name: 'Solo' }]]);
  vm.runInContext('addGameApps(ids)', c);
  c.byId = (id) => c.ids.get(id);
  c.list = [COG, c.ids.get('solo')];
  return c;
}

test('Discover lists a recomp that carries several games as its games', () => {
  const c = discoverCtx();
  assert.equal(vm.runInContext('withGames(list).map((a) => a.id).join()', c), 'cogbox--gow1,cogbox--gow2,cogbox--gow3,cogbox--gowj,solo');
});

test('a game hidden from Discover is left out, and the others stay', () => {
  const c = discoverCtx(['cogbox--gow2']);
  assert.equal(vm.runInContext('withGames(list).map((a) => a.id).join()', c), 'cogbox--gow1,cogbox--gow3,cogbox--gowj,solo');
});

test('Discover counts a recomp\'s games as one recomp', () => {
  const c = discoverCtx();
  assert.equal(vm.runInContext('recompsIn(withGames(list))', c), 2);
});

test('a game\'s page says which recomp it\'s part of and how many games that holds', () => {
  const c = discoverCtx();
  c.LAYERS = '';
  vm.runInContext(fn(app, 'partOfLine'), c);
  const pill = vm.runInContext("partOfLine(byId('cogbox--gow1'))", c);
  assert.match(pill, /class="ver-pill part-pill"/);
  assert.match(pill, /Part of COGBox/);
  assert.match(pill, /4 games/);
});

test('Discover and Big Picture search find a game inside a recomp by either name', () => {
  assert.match(app, /let list = withGames\(visible\(\)\)/);
  assert.match(app, /a\.name, a\.game, a\.parentName/);
  assert.match(tv, /const apps = withGames\(pool\(\)\)/);
  assert.match(tv, /\$\{a\.parentName \|\| ''\}/);
});

test('Big Picture lists each game on its own and shows the recomp\'s games on its page', () => {
  assert.match(tv, /const installed = \(\) => libRecs\(\)/);
  assert.match(tv, /Includes \{n\} Games/);
  assert.match(tv, /data-go-game="\$\{esc\(a\.parent\)\}"/);
  assert.match(app, /const played = jumpRecs\(\)/);
  assert.match(app, /let list = libRecs\(\)\.map/);
});

test('Home\'s rows list a recomp\'s games; the banner and the weekly chart keep the recomp', () => {
  const home = fn(app, 'pageHome');
  assert.match(home, /const games = withGames\(pool\);/);
  for (const row of ['games.filter(turnkey)', 'recentlyUpdated(games)', 'games.filter((a) => a.dev', '[...games].sort(byNew)']) assert.ok(home.includes(row), row);
  assert.match(home, /trendShelf\(pool\)/);
  assert.match(tv, /const games = withGames\(pool\(\)\);/);
  assert.match(tv, /recentlyUpdated\(games\)/);
  assert.match(tv, /const trend = trendList\(pool\(\)\);/);
});

test('a game of a recomp Easy Setup does is set up as its recomp is', () => {
  const c = discoverCtx();
  c.S.easy = new Set(['cogbox']);
  c.builtinBoot = () => ({});
  c.TURNKEY_HOW = [];
  vm.runInContext(`${app.match(/^const needsSetup = .*$/m)[0]}\n${app.match(/^const turnkey = .*$/m)[0]}`, c);
  assert.equal(vm.runInContext("turnkey(byId('cogbox--gow1'))", c), true);
  assert.equal(vm.runInContext("turnkey(byId('solo'))", c), false);
});

test('a game inside a recomp of several joins its game\'s versions, by full id or by name', () => {
  const idx = fn(app, 'index');
  assert.match(idx, /const named = packGames\.filter\(\(c\) => !\(ti\.pc \|\| \[\]\)\.includes\(c\.id\) && sfold\(c\.name\) === sfold\(ti\.name\)\);/);
  assert.match(idx, /for \(const c of \[\.\.\.listed, \.\.\.named\]\) c\.title = ti\.id;/);
});

test('in the versions menu, a version from a recomp of several says which and how many games it holds', () => {
  const c = discoverCtx();
  vm.runInContext(fn(app, 'packLine'), c);
  assert.equal(vm.runInContext("packLine(byId('cogbox--gow2'))", c), 'Part of COGBox · 4 games');
  assert.equal(vm.runInContext("packLine(byId('solo'))", c), '');
  assert.match(fn(app, 'verRow'), /packTag\(v\)/);
  assert.match(tv, /packLine\(v\), S\.library\[v\.id\]/);
});

test('installing a recomp of several games doesn\'t put all of them in Jump back in', () => {
  const c = ctx();
  install(c);
  c.ids = new Map([['cogbox', COG]]);
  vm.runInContext('addGameApps(ids); expandLib()', c);
  const jump = () => vm.runInContext('jumpRecs().map((g) => g.id).sort().join()', c);
  // gow2 was played: it and the other recomp, not the unplayed games.
  assert.equal(jump(), 'cogbox--gow2,other');
  // None played yet: the one it was installed from, else its first.
  c.S.library['cogbox--gow2'].last_played = 0;
  assert.equal(jump(), 'cogbox--gow1,other');
  c.S.settings.ui.pack_from = { cogbox: 'cogbox--gow3' };
  assert.equal(jump(), 'cogbox--gow3,other');
  // The Library itself still has every game.
  assert.equal(vm.runInContext('libRecs().length', c), 5);
});

test('a recomp of several games asks before its first install, then says where its games went', () => {
  const start = fn(app, 'startInstall');
  assert.match(start, /const from = subOf\(id\) \? id : '';\n\s+id = parentId\(id\);/);
  assert.match(start, /if \(dir === undefined && !S\.library\[id\] && !tag && !anyway && pack\.length\) \{\n\s+if \(!\(await confirmPack\(id, pack\)\)\) return;/);
  // Asked before where it goes, so it's asked once.
  assert.ok(start.indexOf('confirmPack(') < start.indexOf('locationModal('));
  assert.match(fn(app, 'confirmPack'), /tf\('Install \{name\}\?', \{ name \}\)/);
  assert.match(app, /else invoke\('recomp_games'\)\.then\(\(sets\) => \{ const list = gameList\(/);
  const done = fn(app, 'packInstalled');
  assert.match(done, /Its \{n\} games are in your Library/);
  assert.match(done, /setupModal\(from\)/);
});

test('a game\'s page doesn\'t show its recomp\'s game info as its own', () => {
  const c = ctx();
  c.ids = new Map([['cogbox', Object.assign({}, COG, { about: { summary: 'Judgment' } })]]);
  vm.runInContext('addGameApps(ids)', c);
  assert.equal(c.ids.get('cogbox--gow1').about, undefined);
  assert.equal(c.ids.get('cogbox').about.summary, 'Judgment');
});

test('the install dialog shows a game\'s own cover, never its recomp\'s repeated', () => {
  const confirm = fn(app, 'confirmPack');
  assert.match(confirm, /const c = catalogArt\(g, 'cover'\); return `<li>\$\{c \? pic\(thumbOf\(c, 160\)/);
  assert.match(confirm, /: '<span class="ph"><\/span>'/);
  assert.doesNotMatch(confirm, /byId\(id \+ GAME_SEP/);
});
