const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js');
const tv = read('tv.js');

// The two rules behind Set Up Again and its New mark, with the state they read.
const src = app.slice(app.indexOf('// Options its project added since the game was set up'), app.indexOf('// Before it plays: each new option'));
const load = (S) => new Function('S', `${src} return { newPrepOpts, canSetUpAgain };`)(S);
const opts = [{ id: 'hd', new: false }, { id: 'dlss', new: true }];

test('a set-up game lists only the options its player has not answered', () => {
  const { newPrepOpts } = load({});
  assert.deepEqual(newPrepOpts({ copy: 'C:/game.nup', prepare_options: opts }).map((o) => o.id), ['dlss']);
});

test('a game not set up yet has no new options to ask about', () => {
  const { newPrepOpts } = load({});
  assert.deepEqual(newPrepOpts({ copy: null, prepare_options: opts }), []);
  assert.deepEqual(newPrepOpts('loading'), []);
});

// Reaching Extras in Setup Options, with what it saves and the setup info it leaves.
const seen = async () => {
  const calls = [];
  const su = { copy: 'C:/game.nup', prepare_options: [{ id: 'hd', new: true, on: false }] };
  const load2 = new Function('S', 'invoke', 'render', 'window', 'document', `${src} return { seenNewOpts, newPrepOpts };`);
  const { seenNewOpts, newPrepOpts } = load2({ setup: { g: su } }, async (cmd, args) => { calls.push([cmd, args]); }, () => {}, {}, {});
  await seenNewOpts('g', su.prepare_options);
  return { calls, su, newPrepOpts };
};

test('seeing the new options keeps each as it is', async () => {
  const { calls } = await seen();
  assert.deepEqual(calls, [['set_prepare_opts', { id: 'g', opts: { hd: false } }]]);
});

test('seen options no longer count as new, so the dots go', async () => {
  const { su, newPrepOpts } = await seen();
  assert.deepEqual(newPrepOpts(su), []);
});

test('the open setup window keeps its New tag after they are seen', async () => {
  const { su } = await seen();
  assert.equal(su.prepare_options[0].new, true);
});

test('the setup window marks them seen on reaching Extras, once', () => {
  assert.match(read('setup-steps.js'), /if \(m\.cur\.id === 'extras' && m\.fresh\.length && !ns\.seenNew && typeof seenNewOpts === 'function'\) \{\n\s+ns\.seenNew = true;\n\s+seenNewOpts\(id, m\.fresh\);/);
});

test('a game set up from named inputs counts as set up', () => {
  const { newPrepOpts } = load({});
  assert.equal(newPrepOpts({ inputs_ready: true, prepare_options: opts }).length, 1);
});

test('Set Up Again shows only for an Easy Setup game that is set up', () => {
  const S = { easy: new Set(['longshot']), setup: { longshot: { needs: 'Your copy', copy: 'C:/game.nup' } } };
  const { canSetUpAgain } = load(S);
  assert.equal(canSetUpAgain('longshot'), true);
  S.setup.longshot.copy = null;
  assert.equal(canSetUpAgain('longshot'), false);
  assert.equal(canSetUpAgain('other'), false);
});

test('Play asks about new options before the game starts, in both modes', () => {
  assert.match(app, /if \(await askNewOpts\(id\)\) return;\n\s*await playGame2\(id, program\);/);
  assert.match(app, /T\.event\.listen\('options-new'/);
  assert.match(app, /invoke\('take_options_asked'\)/);
  // Big Picture: Set Up Again beside Play, and the options as items the controller can reach.
  assert.match(tv, /btn\('g-again', 'setup', esc\(t\('Set Up Again'\)\)/);
  assert.match(tv, /row\.dataset\.f = ''; row\.dataset\.key = 'po:' \+ o\.id; row\.dataset\.sheet = at;/);
});

test('Big Picture says who made it under Based on, as the desktop page does, not in a chip', () => {
  assert.match(tv, /const byLine = who \? `<p class="tv-based tv-by">\$\{pic\}<span>\$\{htmlIn\(t\('By \{who\}'\)/);
  assert.match(tv, /<p class="tv-based">[^\n]*\n\s+\$\{byLine\}/);
  assert.match(tv, /const chips = \[originName\(a\.origin\), GENRES\[a\.genre\] \? t\(GENRES\[a\.genre\]\) : ''\]\.filter\(Boolean\)\.map\(esc\);\n/);
});

test('Big Picture\'s facts leave out who made it (it is under the title)', () => {
  assert.doesNotMatch(tv, /\[t\('By'\), \(devOf\(a\)/);
});
