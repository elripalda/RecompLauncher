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
