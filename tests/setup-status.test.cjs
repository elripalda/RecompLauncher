const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js');
const tv = read('tv.js');

// The core's answer (game_setup's status) is the one rule for Set up against Play.
const src = app.slice(app.indexOf('// Easy Setup\'s answer for a game, worked out by the core'), app.indexOf('// Turnkey:'));
const load = (S) => new Function('S', `${src} return { setupStatus, easyNotReady };`)(S);

test('a game built from its copy is not asked to set up again when the copy is gone', () => {
  const S = { easy: new Set(['hookshot']), setup: { hookshot: { needs: 'Your copy', copy: null, status: { ready: true, reason: 'built' } } } };
  assert.equal(load(S).easyNotReady('hookshot'), false);
});

test('a game whose setup step changed is set up again, with its copy kept', () => {
  const S = { easy: new Set(['hookshot']), setup: { hookshot: { needs: 'Your copy', copy: 'C:/game.nup', status: { ready: false, reason: 'rebuild' } } } };
  assert.equal(load(S).easyNotReady('hookshot'), true);
});

test('nothing is decided before the core answers, nor for a game Easy Setup does not do', () => {
  const S = { easy: new Set(['hookshot']), setup: { hookshot: 'loading', other: { status: { ready: false, reason: 'copy' } } } };
  const { easyNotReady, setupStatus } = load(S);
  assert.equal(setupStatus('hookshot'), null);
  assert.equal(easyNotReady('hookshot'), false);
  assert.equal(easyNotReady('other'), false);
});

test('the game page and Big Picture both ask the core', () => {
  assert.match(app, /: easyNotReady\(a\.id\) \? b\('main', 'setup'/);
  assert.match(tv, /return easyNotReady\(id\) \? btn\(key, 'setup'/);
  assert.match(tv, /if \(easyNotReady\(id\)\) return \{ label: t\('Set up'\)/);
  // No view works it out from the copy alone any more.
  assert.doesNotMatch(app, /S\.setup\[a\.id\]\.needs && \(!S\.setup\[a\.id\]\.copy/);
  assert.doesNotMatch(tv, /su\.needs && !su\.copy\)/);
});
