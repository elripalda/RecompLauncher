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

test('a better version of a game is suggested only when the catalog says so, and the player decides', () => {
  const start = app.indexOf('function betterOf(a)');
  const src = app.slice(start, app.indexOf('function betterNote', start));
  const S = { library: { old: { id: 'old' } }, settings: { ui: {} } };
  const g = { suggest: true, pick: 'new', list: [] };
  const ids = { old: { id: 'old' }, new: { id: 'new', name: 'New' } };
  const betterOf = new Function('S', 'verGroup', 'byId', 'forOS', `${src} return betterOf;`)(S, () => g, (id) => ids[id], () => true);
  assert.equal(betterOf(ids.old).id, 'new');
  g.suggest = false;
  assert.equal(betterOf(ids.old), null);
  g.suggest = true;
  S.settings.ui.better_off = { old: 'new' };
  assert.equal(betterOf(ids.old), null);
  S.settings.ui.better_off = {};
  S.library.new = { id: 'new' };
  assert.equal(betterOf(ids.old), null);
  // Switching never uninstalls by itself: the old one is offered once the new one is in.
  assert.match(app, /if \(S\.library\[from\] && S\.library\[to\] && !S\.running\.has\(from\)\) confirmUninstall\(from\);/);
});

test("a setup step's options show as ticks, a license one off until the player ticks it", () => {
  const start = app.indexOf('function prepOptsHtml(su)');
  const src = app.slice(start, app.indexOf('document.addEventListener(\'change\'', start));
  const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const htmlFill = (s, v) => esc(s).replace(/\{(\w+)\}/g, (m, k) => (k in v ? String(v[k]) : m));
  const prepOptsHtml = new Function('esc', 't', 'htmlFill', `${src} return prepOptsHtml;`)(esc, (s) => s, htmlFill);
  assert.equal(prepOptsHtml({ prepare_options: [] }), '');
  const out = prepOptsHtml({ prepare_options: [{ id: 'dlss', label: 'Upscaler A', note: 'Sharper.', license: { name: 'A license', url: 'https://example.com/a' }, on: false }, { id: 'hd', label: 'HD', on: true }] });
  assert.match(out, /<h3>Options<\/h3>/);
  assert.match(out, /data-popt="dlss"><span><b>Upscaler A<\/b> <em>\(I accept the <a href="#" data-url="https:\/\/example\.com\/a">A license<\/a>\)<\/em><small>Sharper\.<\/small>/);
  assert.match(out, /data-popt="hd" checked>/);
  // The ticks are kept for the game, and a rebuild can use the copy it was set up with.
  assert.match(app, /invoke\('set_prepare_opts', \{ id, opts \}\)/);
  assert.match(app, /case 'same': return suBegin\(su\.copy\);/);
  assert.match(read('setup-inputs.js'), /prepOptsHtml\(su\)/);
});

test('coming from another launcher gets one window that brings the games over', () => {
  const adopt = read('adopt.js');
  assert.match(adopt, /if \(!ui\.mig_asked\) \{/);
  assert.match(adopt, /function migWelcome\(g\)/);
  assert.match(adopt, /await migAddReady\(\);\n    await migAway\(\);/);
});

test('a compressed copy can be made plain right in the setup window', () => {
  assert.match(app, /else if \(\/\^CONVERT\\\|\/\.test\(SU\.err \|\| ''\)\)/);
  assert.match(app, /data-su="convert">\$\{esc\(t\('Make a Plain Copy'\)\)\}/);
  assert.match(app, /const out = await invoke\('convert_copy', \{ path: src \}\);/);
  assert.match(app, /T\.event\.listen\('convert',/);
});
