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
