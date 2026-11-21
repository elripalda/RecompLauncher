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

function constLine(name) {
  return app.match(new RegExp(`^const ${name} = .*`, 'm'))[0];
}

// tile() and discTile() from app.js, with everything around them reduced to plain stand-ins.
function ctx(ui) {
  const c = vm.createContext({
    S: { settings: { ui }, library: {}, jobs: {}, running: new Set() },
    t: (v) => v, tf: (v) => v, tn: (n, one, many) => (n === 1 ? one : many),
    media: (v) => v, thumbOf: (v) => v, cssUrl: (v) => v, devOf: () => null, status: () => '', isSaved: () => false,
    poster: () => '<poster>', shortName: (v) => v, osMarks: () => '', originName: (v) => v, byId: () => ({}),
    TICK: () => '', PLAY: '', DOWN: '', PLUS: '', LAYERS: '', DISC_S: '',
  });
  for (const name of ['esc', 'verText', 'showVer']) vm.runInContext(constLine(name), c);
  vm.runInContext(fn('tile'), c);
  const disc = app.slice(app.indexOf('const discTile = '), app.indexOf('\n};', app.indexOf('const discTile = ')) + 3);
  vm.runInContext(disc, c);
  return c;
}

const game = { id: 'odyssey', name: 'Odyssey Recomp', version: '0.2.6', origin: 'switch' };

test('a tile shows the version under the name when given one', () => {
  const html = vm.runInContext('tile(game, { ver: "0.2.6" })', Object.assign(ctx({}), { game }));
  assert.match(html, /<span class="s tver"><span>v0\.2\.6<\/span><\/span><span class="s">/);
});

test('a tile has no version line without one', () => {
  const html = vm.runInContext('tile(game, {})', Object.assign(ctx({}), { game }));
  assert.doesNotMatch(html, /tver/);
});

test('a tile keeps an empty version line when the setting is on but there is no version', () => {
  const html = vm.runInContext('tile(game, { ver: "" })', Object.assign(ctx({}), { game }));
  assert.match(html, /<span class="s tver"><span>&nbsp;<\/span><\/span>/);
});

test('Discover leaves the version line off by default', () => {
  const html = vm.runInContext('discTile(game)', Object.assign(ctx({}), { game }));
  assert.doesNotMatch(html, /tver/);
});

test('Discover shows the latest version with the setting on', () => {
  const html = vm.runInContext('discTile(game)', Object.assign(ctx({ show_versions: true }), { game }));
  assert.match(html, /class="s tver"><span>v0\.2\.6</);
});

test('the Library passes the installed release to its tiles only with the setting on', () => {
  const grid = fn('libraryGrid');
  assert.match(grid, /ver: showVer\(\) \? x\.g\.tag \|\| '' : null/);
});

test('a game page before install shows the latest version with its scan right after it', () => {
  const strip = fn('gameStrip');
  assert.match(strip, /ver = esc\(a\.version \|\| '—'\) \+ \(tag \? scanPill\(\{ id, tag \}\) : ''\)/);
});

test('a game page before install looks up the scan by the release tag, not the version', () => {
  const strip = fn('gameStrip');
  assert.match(strip, /const tag = a\.tag \|\| a\.version;/);
});

test('with an update ready, the strip shows where it goes and the update\'s own check', () => {
  const strip = fn('gameStrip');
  assert.match(strip, /scanPill\(\{ id, tag: up, update: true \}\)/);
});

test('a Library game that is only saved shows its latest version', () => {
  const grid = fn('libraryGrid');
  assert.match(grid, /x\.saved \? tile\(x\.a, \{ wide, w, ver: showVer\(\) \? x\.a\.version \|\| '' : null/);
});

test('the Downloads page keeps its number boxes styled after the game page moved to its own strip', () => {
  const css = require('fs').readFileSync(require('path').join(__dirname, '..', 'ui', 'app.css'), 'utf8');
  assert.match(css, /\.stats\{display:grid;/);
  assert.match(css, /\.stats>div\{[^}]*border:1px solid var\(--line\)/);
});
