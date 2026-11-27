// The Easy Setup tag, filter and sorting count only Easy Setup from the project's own
// recomplauncher.json. One the launcher does from its own boot profile still sets up, untagged.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

function context() {
  const c = vm.createContext({
    S: { library: {}, supLocal: {}, easy: new Set(), easyAll: new Set() },
    turnkey: () => true, builtinBoot: () => ({}), TURNKEY_HOW: ['folder', 'flag', 'env', 'setup'],
  });
  vm.runInContext(app.match(/^const supLocalKey = .*$/m)[0].replace(/^const /, 'globalThis.'), c);
  vm.runInContext(['fileCovers', 'supportSource', 'supportStates', 'projectEasy'].map(fn).join('\n'), c);
  return c;
}
const theirFile = { start: { program: ['game.exe'] }, game: { formats: ['iso'] } };

test('a release with its own Easy Setup is tagged', () => {
  const c = context();
  c.S.easyAll.add('own');
  const a = { id: 'own', launcher: theirFile, support: { sdk: 'release', easy_setup: 'project' } };
  assert.equal(c.projectEasy(a), true);
});

test('Easy Setup from the launcher\'s boot profile is not tagged', () => {
  const c = context();
  c.S.easy.add('ours');
  c.S.easyAll.add('ours');
  const a = { id: 'ours', support: { easy_setup: 'launcher' } };
  assert.equal(c.projectEasy(a), false);
});

test('a game the launcher doesn\'t set up is not tagged, whatever its file says', () => {
  const c = context();
  c.turnkey = () => false;
  c.S.easyAll.add('own');
  const a = { id: 'own', launcher: theirFile, support: { sdk: 'release', easy_setup: 'project' } };
  assert.equal(c.projectEasy(a), false);
});

test('an installed release whose own file is broken is not tagged', () => {
  const c = context();
  c.S.easyAll.add('own');
  c.S.library.own = { path: '/g/own', tag: 'v1', updated_at: 1 };
  c.S.supLocal.own = { key: '/g/own@v1@1', v: { problem: 'bad json' } };
  const a = { id: 'own', launcher: theirFile, support: { sdk: 'release', easy_setup: 'project' } };
  assert.equal(c.projectEasy(a), false);
});

test('a game of a recomp of several follows its recomp', () => {
  const c = context();
  c.S.easyAll.add('set');
  const a = { id: 'set--one', parent: 'set', launcher: theirFile, support: { sdk: 'release', easy_setup: 'project' } };
  assert.equal(c.projectEasy(a), true);
});

test('Big Picture\'s game page pill, Discover chip and both Drop in Your Copy rows go by the tag', () => {
  const tvPage = tv.match(/  function pageGame\(id\) \{[^]*?\n  \}/)[0];
  assert.match(tvPage, /: projectEasy\(a\) \? `<span class="tv-easy">\$\{esc\(t\('Easy Setup'\)\)\}<\/span>`/);
  assert.doesNotMatch(tvPage, /S\.easy\.has\(id\) \? `<span class="tv-easy">/);
  assert.match(tv, /apps\.some\(projectEasy\) \? \[\['easy', N_\('Easy Setup'\)\]\]/);
  const dropIn = 'games.filter((a) => S.easy.has(a.parent || a.id) && projectEasy(a) && !haveAny(a))';
  assert.ok(fn('pageHome').includes(dropIn));
  assert.ok(tv.includes(dropIn));
});

test('what Install and Play do still goes by turnkey, not the tag', () => {
  assert.match(app, /turnkey\(a\) \? b\('main easy', 'install', esc\(t\('Easy Setup'\)\)/);
  assert.match(app, /const askCopy = !selfRun && !building && feedsCopy\(a\);/);
  assert.match(tv, /esc\(turnkey\(a\) \? t\('Easy Setup'\) : t\('Install'\)\)/);
});
