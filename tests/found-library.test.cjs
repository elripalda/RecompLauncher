const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');

function fn(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  return app.slice(start, app.indexOf('\n}', start) + 2);
}
function context(apps, options = {}) {
  const ids = new Map(apps.map(a => [a.id, a]));
  const titles = new Map();
  for (const a of apps.filter(a => a.title)) {
    if (!titles.has(a.title)) titles.set(a.title, { id: a.title, pick: a.id, list: [], every: [] });
    const g = titles.get(a.title);
    g.every.push(a);
    if (a.systems.includes('linux')) g.list.push(a);
    if (a.recommended) g.pick = a.id;
  }
  const S = { owned: Object.fromEntries(apps.map(a => [a.id, { path: '/dumps/disc.iso' }])),
    library: {}, settings: { ui: { saved: options.saved || {} }, hide_ai: true } };
  const c = vm.createContext({ S, byId: id => ids.get(id), index: () => ({ titles }),
    forOS: a => a.systems.includes('linux'), isAI: a => !!a.ai,
    isHidden: id => !!ids.get(id).hidden, byDlConsolesFirst: (a, b) => a.id.localeCompare(b.id) });
  const groupStart = app.indexOf('const verGroup = ');
  vm.runInContext(app.slice(groupStart, app.indexOf('\n};', groupStart) + 3), c);
  vm.runInContext(app.match(/^const haveAny = .*;$/m)[0], c);
  vm.runInContext(app.match(/^const saved = .*;$/m)[0], c);
  vm.runInContext(app.match(/^const isSaved = .*;$/m)[0], c);
  vm.runInContext(fn('leadOf') + fn('oneEach') + fn('foundInFolders'), c);
  c.found = () => Array.from(vm.runInContext('foundInFolders().map(a => a.id)', c));
  return c;
}
const game = (id, extra = {}) => ({ id, name: id, systems: ['linux'], ...extra });

test('Home excludes a saved, uninstalled game that is already in the Library', () => {
  const c = context([game('saved'), game('installed'), game('new')], { saved: { saved: 123 } });
  c.S.library.installed = { id: 'installed' };
  assert.deepEqual(c.found(), ['new']);
  delete c.S.settings.ui.saved.saved;
  assert.deepEqual(c.found(), ['new', 'saved'], 'removing it from the Library makes the found game eligible again');
});

test('saving any version keeps its other found versions out of Home', () => {
  const c = context([game('native', { title: 'same-game' }),
    game('other', { title: 'same-game', systems: ['windows'] }), game('new')], { saved: { other: 123 } });
  assert.deepEqual(c.found(), ['new']);
});

test('found games retain platform, visibility and recommended-version filtering', () => {
  const c = context([game('a', { title: 'one' }), game('b', { title: 'one', recommended: true }),
    game('hidden', { hidden: true }), game('soon', { soon: true }), game('ai', { ai: true }),
    game('windows', { systems: ['windows'] }), game('untitled')]);
  c.S.owned.unknown = { path: '/unknown.iso' };
  assert.deepEqual(c.found(), ['b', 'untitled']);
});

test('desktop and Big Picture Home use the same found-game selection', () => {
  assert.match(fn('pageHome'), /const found = foundInFolders\(\);/);
  assert.match(tv, /const owned = foundInFolders\(\);/);
});

// addOwned with the Add Found Games to the Library switch (found_lib_off is the switch off).
function ownedContext(ui, library = {}) {
  const apps = new Map(['a', 'b', 'mine'].map(id => [id, game(id)]));
  const S = { owned: { a: {}, b: {} }, ownedOk: true, library, settings: { rom_dirs: ['/roms'], ui } };
  const c = vm.createContext({ S, byId: id => apps.get(id), index: () => ({ titles: new Map() }), forOS: () => true,
    saveUi: async (patch) => { S.settings.ui = Object.assign({}, S.settings.ui, patch); } });
  vm.runInContext(app.match(/^const saved = .*;$/m)[0], c);
  vm.runInContext('async ' + fn('addOwned'), c);
  return c;
}
const ids = (o) => Object.keys(o || {}).sort();

test('found games go into the Library while the switch is on', async () => {
  const c = ownedContext({});
  assert.equal(await vm.runInContext('addOwned()', c), 2);
  assert.deepEqual(ids(c.S.settings.ui.saved), ['a', 'b']);
  assert.deepEqual([...c.S.settings.ui.found_saved].sort(), ['a', 'b']);
});

test('found games stay out of the Library while the switch is off', async () => {
  const c = ownedContext({ found_lib_off: true });
  assert.equal(await vm.runInContext('addOwned()', c), 0);
  assert.deepEqual(ids(c.S.settings.ui.saved), []);
});

test('turning the switch off takes out what a look added, but not installed or player-added games', async () => {
  const c = ownedContext({ saved: { a: 1, b: 1, mine: 1 }, found_saved: ['a', 'b'], found_lib_off: true }, { b: { id: 'b' } });
  assert.equal(await vm.runInContext('addOwned()', c), 0);
  assert.deepEqual(ids(c.S.settings.ui.saved), ['b', 'mine']);
  assert.deepEqual([...c.S.settings.ui.found_saved], []);
});
