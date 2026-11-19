const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js');

// A function declaration out of a file, by name.
function fn(src, name) {
  const start = src.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  let i = src.indexOf('{', start), depth = 0;
  for (; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) break;
  }
  return src.slice(start, i + 1);
}

test('the desktop uninstall keeps a copy of the saves unless the tick is taken off', () => {
  const u = fn(app, 'confirmUninstall');
  assert.match(u, /<input type="checkbox" id="u-keep" checked>/);
  assert.match(u, /keep = !!\(\$\('#u-keep', s\) \|\| \{\}\)\.checked/);
  assert.match(u, /invoke\('uninstall', \{ id, deleteFiles: del, runUninstaller: unins, keepSaves: keep \}\)/);
});

test('the desktop keep-saves tick is off while the folder stays', () => {
  const u = fn(app, 'confirmUninstall');
  assert.match(u, /delBox\.addEventListener\('change', \(\) => \{ keepBox\.disabled = !delBox\.checked; \}\)/);
});

test('the desktop says where the saves went, or why the folder stayed', () => {
  const u = fn(app, 'confirmUninstall');
  assert.match(u, /if \(r\.saves_kept\) toast\(t\('A copy of its saves is kept\./);
  assert.match(u, /else if \(del && r\.kept && r\.saves_stuck\) toast\(/);
});

test('Big Picture offers the same: delete keeping a copy, or delete the saves too', () => {
  const u = fn(tv, 'tvUninstall');
  assert.match(u, /const run = \(del, keep = true\) =>/);
  assert.match(u, /invoke\('uninstall', \{ id, deleteFiles: del, keepSaves: keep \}\)/);
  assert.match(u, /label: t\('Uninstall and delete its folder'\), sub: tf\('\{path\}\. A copy of its saves is kept first\.'/);
  assert.match(u, /label: t\('Delete its folder and its saves'\)[^\n]*run: run\(true, false\)/);
  assert.match(u, /if \(r\.saves_kept\) toast\(/);
  assert.match(u, /r\.saves_stuck/);
});

test('every new uninstall text is in every language', () => {
  const texts = [
    'Keep a copy of its saves first, to put back if you install it again',
    "{name} is uninstalled, but its folder stayed: a copy of its saves couldn't be made first. It's in {path}.",
    'A copy of its saves is kept. Install it again and put them back from its Saves tab.',
    '{path}. A copy of its saves is kept first.',
    'Delete its folder and its saves',
  ];
  const dir = path.join(__dirname, '../ui/i18n');
  for (const f of fs.readdirSync(dir).filter((x) => x.endsWith('.json') && x !== 'en-GB.json')) {
    const d = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    for (const s of texts) assert.ok(d[s], `${f}: ${s}`);
  }
});
