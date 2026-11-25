const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const plain = (value) => JSON.parse(JSON.stringify(value));

// Hold the core's answer while the player changes one installation.
function checking(automatic = true) {
  let answer;
  const pending = new Promise((resolve) => { answer = resolve; });
  const starts = [];
  const game = { repo: 'Studio/Game', path: 'C:/Games/Game', tag: 'v1', installed_at: 10, updated_at: 10 };
  const S = {
    library: { chosen: { ...game }, other: { ...game, path: 'C:/Games/Other' } },
    settings: { ui: {} }, running: new Set(), jobs: {}, paused: {}, dlArgs: {}, view: 'library',
  };
  const c = vm.createContext({
    S, updateCheckRevision: 0, invoke: () => pending, window: {},
    document: { body: { classList: { contains: () => false } } },
    holdUpdates() {}, renderSide() {}, render() {}, toast() {},
    updatingLine: () => '', readyLine: () => '', badVersion: () => false,
    byId: (id) => ({ id }), autoUpdates: () => automatic,
    updateGame: (id) => { starts.push(id); }, fail: (error) => { throw error; },
  });
  vm.runInContext(fn('checkUpdates') + '\n' + fn('stillChecked') + '\n' + app.match(/^function dlArgs\(.*$/m)[0], c);
  const done = c.checkUpdates(!automatic);
  return { S, starts, c, finish: async () => { answer({ chosen: 'v2', other: 'v2' }); await done; } };
}

test('an unchanged update response still starts automatic updates', async () => {
  const c = checking();
  await c.finish();
  assert.deepEqual(c.starts, ['chosen', 'other']);
  assert.deepEqual(plain(c.S.updates), { chosen: 'v2', other: 'v2' });
});

test('an older check cannot overwrite a version the player just pinned', async () => {
  const c = checking();
  c.S.library.chosen = { ...c.S.library.chosen, tag: 'v3', pinned: true };
  await c.finish();
  assert.deepEqual(c.starts, ['other']);
  assert.deepEqual(plain(c.S.updates), { other: 'v2' });
});

test('an older check cannot replace a newly selected latest version', async () => {
  const c = checking();
  c.S.library.chosen.tag = 'v3';
  await c.finish();
  assert.deepEqual(c.starts, ['other']);
});

test('a changed installation folder discards only its old update result', async () => {
  const c = checking();
  c.S.library.chosen.path = 'D:/Other/Game';
  await c.finish();
  assert.deepEqual(plain(c.S.updates), { other: 'v2' });
});

test('an installation replaced at the same path discards its old update result', async () => {
  const c = checking();
  c.S.library.chosen.updated_at = 20;
  await c.finish();
  assert.deepEqual(plain(c.S.updates), { other: 'v2' });
});

test('a changed project cannot use the previous project update result', async () => {
  const c = checking();
  c.S.library.chosen.repo = 'Other/Project';
  await c.finish();
  assert.deepEqual(plain(c.S.updates), { other: 'v2' });
});

test('manual checks discard stale results without hiding another game update', async () => {
  const c = checking(false);
  c.S.library.chosen.tag = 'v3';
  await c.finish();
  assert.deepEqual(c.starts, []);
  assert.deepEqual(plain(c.S.updates), { other: 'v2' });
});

test('a pinned game rejects an update result even if its tag did not change', async () => {
  const c = checking();
  c.S.library.chosen.pinned = true;
  await c.finish();
  assert.deepEqual(c.starts, ['other']);
});

test('a version download invalidates its old check before the library refresh arrives', async () => {
  const c = checking();
  c.c.dlArgs('chosen', { id: 'chosen', dir: null, tag: 'v3' });
  await c.finish();
  assert.deepEqual(c.starts, ['other']);
  assert.deepEqual(plain(c.S.updates), { other: 'v2' });
});
