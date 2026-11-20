const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js'), more = read('more.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const line = (src, name) => src.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];
const esc = (s) => String(s == null ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const tf = (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]);

test('cards show only Easy Setup and the versions count', () => {
  const poster = fn(app, 'poster');
  assert.doesNotMatch(poster, /deckb|stb|greatOnDeck|STATE_NAME/);
  const tvPoster = tv.match(/  function poster\(a, key, sub, rank\) \{[^]*?\n  \}/)[0];
  assert.doesNotMatch(tvPoster, /deckb|stb|greatOnDeck|STATE_NAME/);
  assert.match(tvPoster, /turnkey\(a\) \? `<span class="tv-easyb\$\{vers \? ' ico' : ''\}"/);
  // The game page keeps the project's state and Great on Deck.
  assert.match(app, /const chips = \[quiet \? '' : sp, turnkey\(a\)/);
  assert.match(tv, /\$\{deckChip\}\$\{stChip\}/);
});

test("the recommended version says why, in one line", () => {
  assert.match(fn(app, 'verRow'), /rec && g\.why \? `<small class="v-why" title="\$\{esc\(g\.why\)\}">/);
  assert.match(tv, /v\.id === g\.pick \? \(g\.why \? `\$\{t\('Recommended'\)\}: \$\{g\.why\}` : t\('Recommended'\)\)/);
});

test('the score follows the release out now once 3 of its players said, like an app store', () => {
  const c = vm.createContext({});
  vm.runInContext(fn(app, 'runTally') + line(app, 'verNorm') + '\n' + line(app, 'verShown') + '\n' + fn(app, 'scoreOf') + '\nglobalThis.verNorm = verNorm;', c);
  const all = { runs: 0, issues: 9, broken: 0 };
  // Two reports on the new version: every report still counts, and how many it has so far shows.
  let s = c.scoreOf(all, { version: 'v0.1.1', counts: { runs: 2, issues: 0, broken: 0 } });
  assert.equal(s.cur, false);
  assert.equal(s.r.pct('runs'), 0);
  assert.equal(s.v, 'v0.1.1');
  assert.equal(s.l.n, 2);
  // Three: the new version's own score, with every version's under it.
  s = c.scoreOf(all, { version: '0.1.1', counts: { runs: 3, issues: 0, broken: 0 } });
  assert.equal(s.cur, true);
  assert.equal(s.r.pct('runs'), 100);
  assert.equal(s.all.pct('runs'), 0);
  assert.equal(s.v, 'v0.1.1');
  // The site's scores give the counts flat.
  assert.equal(c.scoreOf(all, { version: 'version 2', runs: 3, issues: 1, broken: 0 }).cur, true);
  assert.equal(c.verNorm('v0.1.1'), c.verNorm('0.1.1'));
  assert.match(fn(app, 'runsCard'), /tf\('All versions: \{pct\}%'/);
  assert.match(fn(app, 'runsCard'), /data-act="rep-only"/);
});

test('reports need an account: signed out, the sign-in prompt instead of a question or an error', () => {
  const ask = fn(more, 'askRun');
  assert.match(ask, /if \(!signedIn\(\)\) return asked_for \? signInToReport\(\) : undefined;/);
  assert.match(ask, /const s = needSignIn\(ex\);\n\s*if \(s != null\) \{ close\(\); return signInToReport\(s\); \}/);
  assert.match(ask, /Your name isn\\'t shown with your report\./);
  const c = vm.createContext({});
  vm.runInContext(more.match(/^const needSignIn = .*$/m)[0] + '\nglobalThis.needSignIn = needSignIn;', c);
  assert.equal(c.needSignIn(new Error('NEED_SIGNIN|Sign in first.')), 'Sign in first.');
  assert.equal(c.needSignIn(new Error('Something else')), null);
});

test('the build on this PC is offered under the game\'s box', () => {
  const { stepsContext } = require('./steps-context.cjs');
  const c = stepsContext();
  const su = (x) => Object.assign({ needs: 'x', kind: '', inputs: [{ id: 'base', label: 'Base', role: 'game', file: true, active: true, path: null }], prepare_options: [] }, x);
  // Set Up uses the build already in the game's folder, unless Build It Again.
  const built = c.stepsView('h', su({ prebuilt: '/home/p/Games/Hopper' }), {}, { name: 'H' });
  assert.match(built, /still here, so setup skips the build\. <button type="button" data-ss="rebuild">Build It Again<\/button>/);
});

test('files from before get no Use Them Again line', () => {
  const { stepsContext } = require('./steps-context.cjs');
  const c = stepsContext();
  const su = (x) => Object.assign({ needs: 'x', kind: '', inputs: [{ id: 'base', label: 'Base', role: 'game', file: true, active: true, path: null }], prepare_options: [] }, x);
  assert.doesNotMatch(c.stepsView('h', su({ inputs_ready: false }), {}, { name: 'H' }), /Set up before with|data-ss="relink"/);
});

test('players can be asked where each game goes, Easy Install too', () => {
  assert.match(fn(app, 'startInstall'), /\|\| \(S\.settings\.ui \|\| \{\}\)\.ask_where\)\) \{/);
  assert.match(app, /sw\('ask_where', ui\.ask_where, t\('Ask Where to Install'\), 'ui-toggle'\)/);
  assert.match(fn(app, 'locationModal'), /data-ask-where/);
});
