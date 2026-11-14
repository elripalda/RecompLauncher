const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

// protonHidden out of app.js, run against a game with a Linux version and two Windows-only ones.
function load(over = {}) {
  const start = app.indexOf('const hiddenByProton');
  // app.js has Windows line endings, so the closing brace is found without the line break after it.
  const end = app.indexOf('\n}', app.indexOf('function protonHidden(')) + 2;
  assert.ok(start >= 0 && end > start);
  const linux = { id: 'oot-linux', title: 'oot', systems: ['linux', 'windows'] };
  const hook = { id: 'oot-hook', title: 'oot', systems: ['windows'] };
  const sacred = { id: 'oot-sacred', title: 'oot', systems: ['windows'], ai: true };
  const every = [linux, hook, sacred];
  const S = Object.assign({ os: 'linux', settings: {}, library: {}, builds: {} }, over);
  const forOS = (a) => a.systems.includes(S.os);
  const hasWindowsBuild = (a) => a.systems.includes('windows');
  const index = () => ({ titles: new Map([['oot', { every }]]) });
  const isHidden = () => false;
  const isAI = (a) => !!a.ai;
  const f = new Function('S', 'forOS', 'hasWindowsBuild', 'index', 'isHidden', 'isAI', `${app.slice(start, end)} return { protonHidden, hiddenByProton };`);
  const fns = f(S, forOS, hasWindowsBuild, index, isHidden, isAI);
  return { run: fns.protonHidden, left: fns.hiddenByProton, linux, hook, sacred, S };
}

test('with Proton off, Windows-only versions are reported as hidden', () => {
  const x = load();
  assert.deepEqual(x.run(x.linux).map((v) => v.id), ['oot-hook', 'oot-sacred']);
});

test('with Proton on, nothing is hidden', () => {
  const x = load({ settings: { proton: true } });
  assert.equal(x.run(x.linux).length, 0);
});

test('Hide AI-assisted recomps keeps its own recomps out of the count', () => {
  const x = load({ settings: { hide_ai: true } });
  assert.deepEqual(x.run(x.linux).map((v) => v.id), ['oot-hook']);
});

test('an installed Windows version is not counted as hidden', () => {
  const x = load({ library: { 'oot-hook': {} } });
  assert.deepEqual(x.run(x.linux).map((v) => v.id), ['oot-sacred']);
});

test('on Windows nothing is hidden because of Proton', () => {
  const x = load({ os: 'windows' });
  assert.equal(x.run(x.linux).length, 0);
});

test('Discover leaves out a Windows-only recomp when Proton is off, but not one with a Linux version', () => {
  const x = load();
  assert.equal(x.left(x.hook), true);
  assert.equal(x.left(x.linux), false);
});

test('Discover leaves out nothing when Proton is on', () => {
  const x = load({ settings: { proton: true } });
  assert.equal(x.left(x.hook), false);
});
