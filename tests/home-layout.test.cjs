const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

function fn(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  return app.slice(start, app.indexOf('\n}', start) + 2);
}
function line(start) {
  const i = app.indexOf(start);
  assert.ok(i >= 0, start);
  return app.slice(i, app.indexOf('\n', i));
}

// Home's order from the website, with every row drawn as its own key, info boxes as [box:id] and
// banners as [banner:id], so a test reads the page as a list.
function ctx(S) {
  const same = (x) => x;
  const c = vm.createContext({
    S: Object.assign({ settings: { ui: {} } }, S),
    esc: same, t: same, accountId: () => '', byId: (id) => (id === 'game' ? { id } : null), media: same, SITE: 'https://site',
    DISC_L: '[disc]', XMARK: 'x', sponsBand: (slot) => `[spot:${slot}]`,
    httpsOnly: (u) => (/^https:\/\/\S+$/i.test(String(u || '')) ? String(u) : '')
  });
  vm.runInContext(line('const HOME_ROWS') + line('const HOME_GO') + line('const homeClosed') + line('const HOME_ICONS = {').replace('{', '{};'), c);
  vm.runInContext(line('const homeIcon'), c);
  vm.runInContext(fn('homePlan') + fn('homeLayout') + fn('homeBoxShows') + fn('homeBoxAct') + fn('homeBox') + fn('homeBanner'), c);
  return c;
}
const ROWS = Object.fromEntries(['played', 'favs', 'ready', 'updated', 'follow', 'found', 'trend', 'new', 'easy', 'news'].map((k) => [k, `[${k}]`]));
function page(home, S = {}) {
  const c = ctx(Object.assign({ home }, S));
  c.rows = Object.fromEntries(Object.entries(ROWS).map(([k, v]) => [k, () => v]));
  return vm.runInContext('homeLayout(rows)', c);
}

test('Without the website\'s list, Home keeps its order and the App Home Banner after Ready to Play', () => {
  assert.equal(page(null), '[played][favs][ready][spot:app_home][updated][follow][found][trend][new][easy][news]');
});

test('The website\'s list sets the order', () => {
  const items = ['trend', 'played', 'favs', 'ready', 'updated', 'follow', 'found', 'new', 'easy', 'news'].map((id) => ({ k: 'row', id }));
  assert.match(page({ v: 1, items, hide: [] }), /^\[trend\]\[played\]/);
});

test('A hidden row stays out', () => {
  const items = ['played', 'favs', 'ready', 'updated', 'found', 'trend', 'new', 'easy', 'news'].map((id) => ({ k: 'row', id }));
  assert.doesNotMatch(page({ v: 1, items, hide: ['follow'] }), /\[follow\]/);
});

test('A row the list doesn\'t name goes after the row it usually follows', () => {
  const items = ['played', 'favs', 'ready', 'updated', 'follow', 'found', 'new', 'easy', 'news'].map((id) => ({ k: 'row', id }));
  assert.match(page({ v: 1, items, hide: [] }), /\[found\]\[trend\]\[new\]/);
});

test('An item of a kind this version doesn\'t know is skipped', () => {
  const items = [{ k: 'row', id: 'played' }, { k: 'carousel', id: 3 }, { k: 'row', id: 'nope' }];
  assert.match(page({ v: 1, items, hide: [] }), /^\[played\]\[favs\]/);
});

test('An info box shows its words and a link button', () => {
  const html = page({ v: 1, items: [{ k: 'box', id: 4, title: 'Join us', text: 'Words', icon: 'heart', btn: 'Join', act: 'link', arg: 'https://discord.gg/x' }], hide: Object.keys(ROWS) });
  assert.match(html, /<b dir="auto">Join us<\/b>/);
  assert.match(html, /data-act="note-url" data-url="https:\/\/discord.gg\/x">Join</);
});

test('An info box\'s button with a link that isn\'t https is left out', () => {
  const html = page({ v: 1, items: [{ k: 'box', id: 4, title: 'Join us', btn: 'Join', act: 'link', arg: 'javascript:alert(1)' }], hide: Object.keys(ROWS) });
  assert.doesNotMatch(html, /<button/);
});

test('An info box for players with no ROM folder hides once one is added', () => {
  const home = { v: 1, items: [{ k: 'box', id: 4, title: 'Find your games', when: 'no_folders' }], hide: Object.keys(ROWS) };
  assert.match(page(home, { settings: { ui: {}, rom_dirs: [] } }), /Find your games/);
  assert.equal(page(home, { settings: { ui: {}, rom_dirs: ['D:/roms'] } }), '');
});

test('A closed info box stays closed', () => {
  const home = { v: 1, items: [{ k: 'box', id: 4, title: 'Hi', close: true }], hide: Object.keys(ROWS) };
  assert.equal(page(home, { settings: { ui: { home_closed: [4] } } }), '');
});

test('A banner from the list is drawn with its booking, so its views and clicks count', () => {
  const html = page({ v: 1, items: [{ k: 'banner', id: 1, slot: 'app_home', sp: { id: 9, img: '/media/sponsors/a.png', alt: 'Ad', house: false } }], hide: Object.keys(ROWS) });
  assert.match(html, /data-act="sp-go" data-id="9" data-sp="9"/);
  assert.match(html, /src="https:\/\/site\/media\/sponsors\/a.png"/);
});
