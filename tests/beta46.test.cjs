const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js');

// The notes reader, with what it uses from the page.
const mdSrc = app.slice(app.indexOf('const MD_ALERT'), app.indexOf('// Titles never wrap'));
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const md = new Function('esc', 't', 'N_', 'tn', `${mdSrc} return md;`)(esc, (s) => s, (s) => s, (n, a, b) => (n === 1 ? a : b).replace('{n}', n));

test('release notes show their tables as tables', () => {
  const out = md('| File | For |\n|---|---|\n| a.zip | Windows |\n| b.zip | Linux |');
  assert.match(out, /<table><thead><tr><th>File<\/th><th>For<\/th><\/tr><\/thead><tbody><tr><td>a\.zip<\/td><td>Windows<\/td><\/tr>/);
  assert.doesNotMatch(out, /\|/);
  // Outer pipes are optional.
  assert.match(md('File | For\n--- | ---\na.zip | Windows'), /<td>a\.zip<\/td><td>Windows<\/td>/);
  // A line of dashes on its own is still a rule, not a table.
  assert.doesNotMatch(md('one | two\n---\nmore'), /<table>/);
});

test('long tables in update notes start folded', () => {
  const rows = Array.from({ length: 5 }, (_, i) => `| f${i} | x |`).join('\n');
  assert.match(md(`| File | For |\n|---|---|\n${rows}`, false, { foldTables: true }), /<details class="md-fold"><summary>A table of 5 rows<\/summary>/);
  assert.doesNotMatch(md(`| File | For |\n|---|---|\n${rows}`), /<details/);
});

test("GitHub's note boxes become boxes, and a quote keeps its lines together", () => {
  const out = md('> [!IMPORTANT]\n> No game files are included.\n> Bring your own.');
  assert.match(out, /<div class="md-alert important"><b>Important<\/b><p>No game files are included\. Bring your own\.<\/p><\/div>/);
  assert.equal(md('> one\n> two'), '<blockquote><p>one two</p></blockquote>');
});

test('the History notes preview long notes with Show All and a way to the game', () => {
  assert.match(app, /function updClamp\(html, u\)/);
  assert.match(app, /data-act="upd-all" hidden>/);
  assert.match(app, /case 'upd-game': S\.nextTab = 'updates'; return go\('game', id\);/);
  assert.match(read('titlecase.js'), /, \.md';/);
});

test('the versions list marks the recommended and installed versions', () => {
  assert.match(app, /\$\{rec \? ' rec' : ''\}\$\{inst \? ' inst' : ''\}/);
  assert.match(read('app.css'), /\.vm-row\.rec,\.vc-row\.rec\{background:linear-gradient/);
});

test('a Python game without Python gets a window with Get Python', () => {
  assert.match(app, /\^NEED_PYTHON\\\|/);
  assert.match(app, /https:\/\/www\.python\.org\/downloads\//);
});

test('a build that went missing opens Easy Setup with a note', () => {
  assert.match(app, /\^NEED_BUILD\\\|/);
  assert.match(app, /byhandoff: \[N_\('Use the Launcher\\'s Way'\)/);
});

test('Discover can list games instead of recomps', () => {
  assert.match(app, /const byGame = \(\) => \(\(S\.settings\.ui \|\| \{\}\)\.disc_by \|\| ''\) === 'games';/);
  assert.match(app, /data-act="disc-by"/);
  // A game's tiles are drawn again when the view changes, not kept from the other view.
  assert.match(app, /<div class="grid\$\{byGame\(\) \? ' by-game' : ''\}"/);
});

test("Discover's lists show each console's dot and each system's icon", () => {
  assert.match(app, /window\.dropIcon = \(k\) =>/);
  assert.match(app, /\[o\.id, o\.name, 'c:' \+ o\.id\]/);
  assert.match(read('drops.js'), /o\.dataset\.ico/);
  assert.match(read('app.css'), /@media \(min-width:1500px\)\{\.dctl\.rows\{flex-direction:row/);
});
