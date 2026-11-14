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

function library(dpr = 1) {
  const S = { view: 'library', settings: { ui: { tile: 168 } }, library: {}, owned: {}, jobs: {},
    site: 'https://recomplauncher.com', thumbs: 'https://recompstore.com/api/thumb.php' };
  const a = { id: 'game', name: 'Game', cover: '/media/game.jpg' };
  const timers = new Map(), saved = [], draws = [];
  let input, next = 0, html;
  const box = { style: { setProperty() {} } };
  const c = vm.createContext({ S, devicePixelRatio: dpr, LOADED: new Set(),
    T: { core: {} }, document: { addEventListener: (_, handler) => { input = handler; } },
    $: () => box, flipGrid: (_, change) => change(), evenGrids() {}, setClock() {},
    setTimeout: callback => { timers.set(++next, callback); return next; },
    clearTimeout: id => timers.delete(id),
    invoke: async (_, args) => { saved.push(args.settings.ui.tile); }, fail: error => { throw error; },
    libList: () => [{ a, saved: true }], t: value => value,
    status: () => '', isSaved: () => false, turnkey: () => false,
    verGroup: () => null, greatOnDeck: () => false, projectState: () => null,
    render: () => { html = vm.runInContext('libraryGrid()', c); draws.push(html); }
  });
  for (const name of ['esc', 'webUrl', 'THUMB_RE', 'thumbOf', 'tileThumb', 'media', 'pic', 'libTile', 'libWide', 'saveUi']) {
    vm.runInContext(app.match(new RegExp(`^const ${name} = .*`, 'm'))[0], c);
  }
  vm.runInContext(fn('poster') + fn('libraryGrid') + 'let settingsSaving = Promise.resolve();' + fn('saveSettings'), c);
  // Only the tile's button wrapper is omitted; its image URL comes from the real poster renderer.
  vm.runInContext('const tile = (a, o) => poster(a, o.mine, o.inst, o.wide, o.w);', c);
  const start = app.indexOf('let qTimer, tileTimer;');
  vm.runInContext(app.slice(start, app.indexOf("$('#isl').addEventListener", start)), c);
  html = vm.runInContext('libraryGrid()', c);
  return {
    resize: value => input({ target: { id: '', dataset: { act: 'tile-size' }, value } }),
    settle: async () => { for (const callback of timers.values()) callback(); timers.clear(); await vm.runInContext('settingsSaving', c); },
    html: () => html, saved, draws
  };
}

test('enlarging Library covers loads the larger image after the slider settles', async () => {
  const l = library();
  assert.match(l.html(), /w=160/);
  l.resize(360);
  await l.settle();
  assert.match(l.html(), /w=400/);
  assert.deepEqual(l.saved, [360]);
});

test('high-DPI Library covers upgrade once after several slider movements', async () => {
  const l = library(2);
  assert.match(l.html(), /w=400/);
  l.resize(240);
  l.resize(300);
  l.resize(360);
  assert.equal(l.draws.length, 0);
  await l.settle();
  assert.match(l.html(), /w=640/);
  assert.equal(l.draws.length, 1);
  assert.deepEqual(l.saved, [360]);
});
