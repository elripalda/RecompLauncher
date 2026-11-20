const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// Settings → Games and folders → Folder Structure (ui/folders.js). The path rules themselves are
// tested in Rust (folders.rs, folders_move.rs); these check the page's wiring.
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const html = fs.readFileSync(path.join(__dirname, '../ui/index.html'), 'utf8');
const src = fs.readFileSync(path.join(__dirname, '../ui/folders.js'), 'utf8');

function context(layout) {
  const c = vm.createContext({
    S: { view: 'settings', settings: {}, library: {} }, window: {},
    t: (s) => s, N_: (s) => s, esc: String, tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]), tn: (n, a, b) => (n === 1 ? a : b.replace('{n}', n)),
    T: { event: { listen: () => {} } }, invoke: async () => null, render: () => {}, toast: () => {}, fail: (e) => { throw e; },
  });
  vm.runInContext(src, c);
  // FS is a const of the page script, so it's reached through the script's own scope.
  vm.runInContext('FS', c).data = { layout, presets: [{ id: 'console-game-recomp', name: 'Console → Game → Recompilation' }], undo: false };
  return c;
}
const row = (b, s, ctl) => `<row ${b}>${ctl}</row>`;
const group = (title, inner) => `<group ${title}>${inner}</group>`;
const sw = (k, on) => `<sw ${k}=${on}>`;
const level = (f) => ({ parts: [{ field: f }], sep: ' - ' });

test('the page loads the designer and Settings draws its group', () => {
  assert.match(html, /<script src="folders.js"><\/script>/);
  assert.match(html, /<link rel="stylesheet" href="folders.css">/);
  assert.match(app, /typeof folderStructureGroup === 'function' \? folderStructureGroup\(group, row, sw\) : ''/);
  assert.match(app, /if \(\/\^fs-\/\.test\(a\) && window\.foldersAct\) return window\.foldersAct\(a, el\);/);
});

test('the group shows the structure and its new-installs switch, off by default', () => {
  const c = context({ preset: 'console-game-recomp', levels: [level('console'), level('game'), level('recomp')], apply_new: false });
  const out = c.folderStructureGroup(group, row, sw);
  assert.match(out, /Console → Game Title → Recompilation/);
  assert.match(out, /<sw fs_new=false>/);
  assert.match(out, /data-act="fs-reorg"/);
  assert.doesNotMatch(out, /fs-undo/);
});

test('a combined level reads as its fields joined', () => {
  const c = context({ preset: '', levels: [{ parts: [{ field: 'game' }, { field: 'text', text: 'mine' }], sep: ' - ' }], apply_new: true });
  assert.match(c.folderStructureGroup(group, row, sw), /Game Title \+ "mine"[^]*Your own[^]*<sw fs_new=true>/);
});

test('the preview tree shows shared folders once', () => {
  const c = context({ levels: [] });
  const tree = c.fsTree([
    { library: 'L', folders: ['N64', 'Majora\'s Mask', 'Zelda64Recomp'] },
    { library: 'L', folders: ['N64', 'Majora\'s Mask', '2 Ship 2 Harkinian'] },
  ]);
  assert.equal((tree.match(/>N64\//g) || []).length, 1);
  assert.equal((tree.match(/class="fs-node game"/g) || []).length, 2);
});
