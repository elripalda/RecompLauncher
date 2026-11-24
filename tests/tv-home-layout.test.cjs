const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');

// A top-level function of app.js, or one inside tv.js (two spaces in), as its source.
function fnApp(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  return app.slice(start, app.indexOf('\n}', start) + 2);
}
function fnTv(name) {
  const start = tv.indexOf(`  function ${name}(`);
  assert.ok(start >= 0, name);
  return tv.slice(start, tv.indexOf('\n  }', start) + 4);
}
function line(src, start) {
  const i = src.indexOf(start);
  assert.ok(i >= 0, start);
  return src.slice(i, src.indexOf('\n', i));
}

// Big Picture's Home under the dock, with every row drawn as [its name], info boxes and banners as
// their HTML, so a test reads the page as a list. Every row has games to show.
function ctx(S) {
  const same = (x) => x;
  const games = [{ id: 'a' }, { id: 'b' }, { id: 'c' }, { id: 'd' }];
  const c = vm.createContext({
    S: Object.assign({ settings: { ui: {} }, easy: new Set(['a', 'b', 'c', 'd']), news: [{ id: 'a', name: 'A', tag: 'v1', title: 'A v1', date: '' }], library: {} }, S),
    esc: same, t: same, tf: same, accountId: () => '', byId: (id) => (id === 'game' || id === 'a' ? { id } : null), media: same, SITE: 'https://site',
    DISC_L: '[disc]', svg: () => '[x]',
    httpsOnly: (u) => (/^https:\/\/\S+$/i.test(String(u || '')) ? String(u) : ''),
    row: (name) => `[${name}]`, poster: () => '', card: () => '', timeLine: () => '', date: () => '', ago: () => '',
    recent: () => games.map((a) => ({ a })), foundInFolders: () => games, oneEach: same, turnkey: () => true, byNew: () => 0,
    haveAny: () => false, recentlyUpdated: same, turnkeyFirst: same, trendList: () => [], pool: () => games, notPC: () => true, byDl: () => 0,
    HOME_ROW: 14,
  });
  vm.runInContext(line(app, 'const HOME_ROWS') + line(app, 'const HOME_GO') + line(app, 'const homeClosed') + line(app, 'const HOME_ICONS = {').replace('{', '{};'), c);
  vm.runInContext(line(app, 'const homeIcon'), c);
  vm.runInContext(fnApp('homePlan') + fnApp('homeBoxShows') + fnApp('homeBoxAct'), c);
  vm.runInContext(line(tv, 'const TV_HOME_ROWS') + fnTv('tvHomeRows') + fnTv('tvHomeBox') + fnTv('tvHomeBanner'), c);
  return c;
}
function page(home, kind = 'mine', S = {}) {
  const c = ctx(Object.assign({ home }, S));
  c.dock = { kind };
  return vm.runInContext('tvHomeRows(dock, pool()).join("")', c);
}
const ALL = ['played', 'favs', 'ready', 'updated', 'follow', 'found', 'trend', 'new', 'easy', 'news'];

test('Without the website\'s list, Big Picture\'s Home keeps its own order', () => {
  assert.equal(page(null), '[own][ready][disc][news][upd][new][top]');
});

test('With your pinned games in the dock, Jump back in is the row of the games you played', () => {
  assert.equal(page(null, 'pinned'), '[cont][own][ready][disc][news][upd][new][top]');
});

test('The website\'s list sets Big Picture\'s order too', () => {
  const items = ['trend', 'played', 'favs', 'ready', 'updated', 'follow', 'found', 'new', 'easy', 'news'].map((id) => ({ k: 'row', id }));
  assert.equal(page({ v: 1, items, hide: [] }), '[top][ready][upd][own][new][disc][news]');
});

test('A row hidden on the website stays out of Big Picture', () => {
  const items = ALL.filter((id) => id !== 'news').map((id) => ({ k: 'row', id }));
  assert.doesNotMatch(page({ v: 1, items, hide: ['news'] }), /\[news\]/);
});

test('Rows only the desktop has (Favorites, From developers you follow) are skipped', () => {
  const items = [{ k: 'row', id: 'favs' }, { k: 'row', id: 'follow' }];
  assert.equal(page({ v: 1, items, hide: ALL.filter((id) => id !== 'favs' && id !== 'follow') }), '');
});

test('An info box shows in Big Picture with a button the controller can reach', () => {
  const html = page({ v: 1, items: [{ k: 'box', id: 4, title: 'Join us', text: 'Words', icon: 'heart', btn: 'Join', act: 'link', arg: 'https://discord.gg/x' }], hide: ALL });
  assert.match(html, /<b dir="auto">Join us<\/b><p dir="auto">Words<\/p>/);
  assert.match(html, /data-f data-key="hb:4:go" data-tvact="hbgo" data-id="4"><span>Join</);
  assert.doesNotMatch(html, /still/);
});

test('An info box\'s button with a link that isn\'t https is left out, and the box shows with the row under it', () => {
  const html = page({ v: 1, items: [{ k: 'box', id: 4, title: 'Join us', btn: 'Join', act: 'link', arg: 'javascript:alert(1)' }], hide: ALL });
  assert.doesNotMatch(html, /<button/);
  assert.match(html, /class="tv-sec tv-hbox still"/);
});

test('An info box that can be closed has a close button', () => {
  const html = page({ v: 1, items: [{ k: 'box', id: 4, title: 'Hi', close: true }], hide: ALL });
  assert.match(html, /data-key="hb:4:x" data-tvact="hbclose" data-id="4"/);
});

test('A closed info box stays closed in Big Picture', () => {
  const home = { v: 1, items: [{ k: 'box', id: 4, title: 'Hi', close: true }], hide: ALL };
  assert.equal(page(home, 'mine', { settings: { ui: { home_closed: [4] } } }), '');
});

test('An info box for players with no ROM folder hides once one is added', () => {
  const home = { v: 1, items: [{ k: 'box', id: 4, title: 'Find your games', when: 'no_folders' }], hide: ALL };
  assert.match(page(home, 'mine', { settings: { ui: {}, rom_dirs: [] } }), /Find your games/);
  assert.equal(page(home, 'mine', { settings: { ui: {}, rom_dirs: ['D:/roms'] } }), '');
});

test('A banner from the list is a tile A opens, counted like the desktop\'s', () => {
  const html = page({ v: 1, items: [{ k: 'banner', id: 1, slot: 'app_home', sp: { id: 9, img: '/media/sponsors/a.png', alt: 'Ad', house: false } }], hide: ALL });
  assert.match(html, /data-f data-key="hbn:9" data-tvact="spgo" data-id="9" data-sp="9"/);
  assert.match(html, /src="https:\/\/site\/media\/sponsors\/a.png"/);
});

test('A banner with nothing booked in it isn\'t drawn', () => {
  assert.equal(page({ v: 1, items: [{ k: 'banner', id: 1, slot: 'hb1' }], hide: ALL }), '');
});

test('A box\'s button does the same thing in Big Picture and on the desktop', () => {
  const c = ctx({});
  assert.deepEqual({ ...vm.runInContext('homeBoxAct({ btn: "Go", act: "go:library" })', c) }, { act: 'go', view: 'library' });
  assert.deepEqual({ ...vm.runInContext('homeBoxAct({ btn: "Go", act: "game", arg: "a" })', c) }, { act: 'game', id: 'a' });
  assert.equal(vm.runInContext('homeBoxAct({ btn: "Go", act: "game", arg: "nope" })', c), null);
  assert.equal(vm.runInContext('homeBoxAct({ act: "rom-add" })', c), null);
});

test('Big Picture\'s Home asks for the list again when it opens, and views of its banners count', () => {
  assert.match(tv, /if \(view === 'home'\) homeMaybe\(\);/);
  assert.match(tv, /if \(v === 'home'\) sponsWatch\(\);/);
  assert.match(app, /for \(const el of \$\$\('\[data-sp\]'\)\) spObs\.observe\(el\);/);
});
