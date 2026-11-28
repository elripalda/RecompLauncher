const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js');
const tv = read('tv.js');

// The core's answer (game_setup's status) is the one rule for Set up against Play.
const src = app.slice(app.indexOf('// Easy Setup\'s answer for a game, worked out by the core'), app.indexOf('// Turnkey:'));
const load = (S) => new Function('S', 'tf', 'byId', `${src} return { setupStatus, easyNotReady, copyNotNeeded, copyGoneText };`)(S, (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => v[k]), () => null);

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
  assert.match(app, /: easyNotReady\(a\.id\) \? b\('main easy', 'setup'/);
  assert.match(tv, /return easyNotReady\(id\) \? btn\(key, 'setup'/);
  assert.match(tv, /if \(easyNotReady\(id\)\) return \{ label: t\('Easy Setup'\)/);
  // No view works it out from the copy alone any more.
  assert.doesNotMatch(app, /S\.setup\[a\.id\]\.needs && \(!S\.setup\[a\.id\]\.copy/);
  assert.doesNotMatch(tv, /su\.needs && !su\.copy\)/);
});

test('Easy Setup tells the core when its window opens and closes, in both modes', () => {
  assert.match(app, /const setupOpened = \(id\) => \{ if \(id\) setupClosing = setupClosing\.then\(\(\) => invoke\('setup_begin', \{ id \}\)\)/);
  assert.match(app, /const setupClosed = \(id\) => \{ if \(id\) setupClosing = setupClosing\.then\(\(\) => invoke\('setup_end', \{ id \}\)\)/);
  // Opened: the window with a run going, the setup steps (every game's files), Big Picture's sheet.
  assert.equal((app.match(/SU\.el = s;\r?\n\s*setupOpened\(id\);/g) || []).length, 2);
  assert.match(tv, /if \(TS\.id !== id\) setupOpened\(id\);/);
  // Closed: as it closes, sent to the background, or cancelled.
  assert.match(app, /SU\.el = null;\r?\n\s*setupClosed\(closingId\);/);
  assert.match(app, /function suBackground\(\) \{\r?\n\s*suDrive\.stop\(\); suTickStop\(\);\r?\n[^\n]*\r?\n\s*setupClosed\(id\);/);
  assert.match(app, /invoke\('setup_cancel', \{ id \}\)\.catch\(\(\) => \{\}\);\r?\n\s*setupClosed\(id\);/);
  assert.match(tv, /setupClosed\(id\);\r?\n\s*TS\.id = null;/);
});

test('a game built from a copy that is gone says it plays without it, instead of asking', () => {
  const built = { needs: 'Your copy', copy: null, status: { ready: true, reason: 'built' } };
  const S = { easy: new Set(['hookshot']), library: { hookshot: { name: 'Hookshot' } }, setup: { hookshot: built } };
  const { copyNotNeeded, copyGoneText } = load(S);
  assert.equal(copyNotNeeded('hookshot'), true);
  assert.equal(copyGoneText('hookshot'), "Your copy isn't here anymore. Hookshot doesn't need it: it's already built.");
  // Its copy still there, or a game that isn't set up: not this case.
  S.setup.hookshot = Object.assign({}, built, { copy: 'D:/hookshot.nup' });
  assert.equal(copyNotNeeded('hookshot'), false);
  S.setup.hookshot = Object.assign({}, built, { status: { ready: false, reason: 'copy' } });
  assert.equal(copyNotNeeded('hookshot'), false);
});

test('every copy row uses it: Before You Play, the Manage tab and Big Picture', () => {
  assert.match(app, /d: copyGoneText\(id\)/);
  // Manage tab: said, and picking a copy isn't the main button then.
  assert.match(app, /const gone = copyNotNeeded\(id\), have = su\.copy \|\| gone;\r?\n\s+const sub = su\.copy \? su\.copy : gone \? copyGoneText\(id\) : su\.needs;/);
  assert.match(app, /class="btn\$\{have \? '' : ' main'\}" data-act="setup"/);
  assert.match(tv, /const gone = copyNotNeeded\(id\), have = prof\.copy \|\| gone;/);
  assert.match(tv, /const gone = copyNotNeeded\(TV\.arg\);/);
});
