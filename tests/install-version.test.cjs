// #393: an install with no version goes to the one the store let out. Longshot's store version
// is v3.0.0 while its newest releases are nightlies, so every install that means "this version"
// or "the update on offer" names it.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

function context(extra) {
  const c = vm.createContext(Object.assign({
    S: { library: {}, updates: {}, jobs: {}, dlArgs: {}, uninstalling: new Set() },
    t: (x) => x,
    tf: (x, v) => x.replace(/\{(\w+)\}/g, (_, k) => v[k]),
    esc: (x) => String(x),
    parentId: (id) => id,
    window: {},
  }, extra));
  vm.runInContext(app.match(/^const relKey = .*$/m)[0].replace(/^const /, 'globalThis.'), c);
  return c;
}

const nightlies = [{ tag_name: 'nightly-681' }, { tag_name: 'nightly-680' }, { tag_name: 'v3.0.0' }];

test('an update installs the version on offer by name', () => {
  const asked = [];
  // (A game that builds on Play: its update starts at once, without waiting its turn.)
  const c = context({ startInstall: (...args) => { asked.push(args); return Promise.resolve(); }, buildsAfterUpdate: () => false });
  vm.runInContext(fn('updateGame'), c);
  c.S.updates.longshot = 'nightly-681';
  c.updateGame('longshot');
  assert.deepEqual(asked[0], ['longshot', undefined, 'nightly-681']);
});

test('automatic updates, Update All and the Update buttons all go through updateGame', () => {
  assert.match(app, /for \(const id of auto\) updateGame\(id\);/);
  assert.match(app, /for \(const x of ids\) updateGame\(x\);/);
  assert.match(app, /case 'install': return S\.library\[parentId\(id\)\] && S\.updates\[parentId\(id\)\] \? updateGame\(id\) : startInstall\(id\);/);
  assert.match(tv, /run: \(\) => updateGame\(id\)/);
});

test('a version picked in Big Picture is installed by name, the newest too', () => {
  assert.doesNotMatch(tv, /v\.newest \? null : v\.tag/);
  assert.match(tv, /startInstall\(id, undefined, v\.tag\)/);
});

test('a picked nightly says it goes back to the store version', () => {
  const c = context();
  vm.runInContext(fn('pinnedCard'), c);
  const card = c.pinnedCard('longshot', { tag: 'nightly-680' }, nightlies, 'v3.0.0');
  assert.match(card, /Go Back to v3\.0\.0/);
  assert.match(card, /data-act="version" data-tag="v3\.0\.0"/);
});

test('a picked older version goes to the newest', () => {
  const c = context();
  vm.runInContext(fn('pinnedCard'), c);
  const card = c.pinnedCard('mkw', { tag: 'v0.1.10' }, [{ tag_name: 'v0.1.12' }, { tag_name: 'v0.1.10' }], 'v0.1.12');
  assert.match(card, /Go to the newest/);
  assert.match(card, /data-tag="v0\.1\.12"/);
});

// pickBuild's own install, through Big Picture's picker (which calls back with the file).
function pickedInstall(setup) {
  const sent = [];
  const c = context({
    render: () => {},
    dlArgs: (id, args) => { c.S.dlArgs[id] = args; },
    invoke: (cmd, args) => { sent.push([cmd, args]); return Promise.resolve(); },
    byId: () => null,
  });
  c.window.tvPickBuild = (id, builds, current, go) => { go('Longshot.AppImage'); return true; };
  setup(c.S);
  vm.runInContext(fn('pickBuild'), c);
  c.pickBuild('longshot', [{ name: 'Longshot.AppImage' }], '');
  return sent[0][1];
}

test('changing the build keeps the version installed', () => {
  const args = pickedInstall((S) => { S.library.longshot = { tag: 'nightly-680' }; });
  assert.equal(args.tag, 'nightly-680');
  assert.equal(args.dir, null);
});

test('picking the build of a first install keeps its version and folder', () => {
  const args = pickedInstall((S) => { S.dlArgs.longshot = { id: 'longshot', dir: 'D:/Games', tag: 'nightly-681' }; });
  assert.equal(args.tag, 'nightly-681');
  assert.equal(args.dir, 'D:/Games');
});
