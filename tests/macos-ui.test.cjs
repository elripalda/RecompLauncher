const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

// Exercise the real picker logic without requiring a webview or invoking a native dialog.
const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
function picker(os, opts, entry) {
  const ctx = vm.createContext({
    S: { os }, FB: { opts, sel: 0, path: '/Games' },
    fbRows: () => entry ? [entry] : [],
  });
  for (const name of ['fbProgramBundle', 'fbTakes', 'fbChoice']) {
    const match = source.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'));
    if (match) vm.runInContext(match[0], ctx);
  }
  return vm.runInContext('fbChoice()', ctx);
}

const app = { name: 'My Game.app', path: '/Games/My Game.app', dir: true };
test('Choose Program can select a Mac app bundle', () => {
  assert.equal(picker('macos', { program: true }, app), app.path);
});
test('a ROM file picker does not accept an app directory as the game copy', () => {
  assert.equal(picker('macos', { filters: [{ extensions: ['z64'] }] }, app), '');
});
test('ordinary directories remain navigable rather than selectable as programs', () => {
  assert.equal(picker('macos', { program: true }, { name: 'Folder', path: '/Games/Folder', dir: true }), '');
  assert.equal(picker('windows', { program: true }, app), '');
  assert.equal(picker('linux', { program: true }, app), '');
});
test('file and folder selection keep their existing behavior', () => {
  assert.equal(picker('windows', { program: true }, { name: 'Game.exe', path: '/Games/Game.exe', dir: false }), '/Games/Game.exe');
  assert.equal(picker('macos', { directory: true }, app), app.path);
  assert.equal(picker('macos', { directory: true }, null), '/Games');
});

// The desktop menu's Add To submenu, as each system shows it.
function addTo(os, steam) {
  const menu = source.match(/^function gameMenu\([^]*?^}/m)[0];
  const items = menu.match(/label: t\('Add To'\), icon: 'plus', sub: (\[[^]*?\n {6}\]) \},/)[1];
  const ctx = { S: { os, steam }, t: (s) => s, id: 'g', favs: () => [] };
  return vm.runInNewContext(items, ctx).map((i) => i.act);
}
test('a Mac has no Add to desktop in the game menu', () => {
  assert.ok(!addTo('macos', false).includes('desktop'));
});
test('Add To keeps Favorites on a Mac', () => {
  assert.ok(addTo('macos', false).includes('fav'));
});
test('Windows and Linux keep Add to desktop in the game menu', () => {
  assert.ok(addTo('windows', false).includes('desktop'));
  assert.ok(addTo('linux', true).includes('desktop'));
});

// Big Picture's shortcut items of a game's menu, as each system shows them.
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
function tvShortcuts(os, steam) {
  const lines = tv.match(/\n( *\/\/ No Add to desktop on a Mac[^\n]*\n[^\n]*\n[^\n]*)\n/)[1];
  const ctx = { S: { os, steam }, t: (s) => s, tf: (s) => s, id: 'g', g: { name: 'Game' } };
  // Copied into an array of this context: deepEqual tells arrays of another context apart.
  return [...vm.runInNewContext(`[${lines}]`, ctx)].map((i) => i.label);
}
test('Big Picture shows no Add to desktop on a Mac', () => {
  assert.deepEqual(tvShortcuts('macos', false), []);
});
test('Big Picture keeps its shortcuts on Windows and Linux', () => {
  assert.deepEqual(tvShortcuts('linux', false), ['Add to desktop']);
  assert.deepEqual(tvShortcuts('windows', true), ['Add to Steam', 'Add to desktop']);
});
