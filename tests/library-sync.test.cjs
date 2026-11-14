const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const library = source.slice(source.indexOf('const saved ='), source.indexOf('const setupDone ='));
const addOwned = source.match(/^async function addOwned\([^]*?^}/m)[0];

function fixture(ui = {}, remote = {}) {
  const S = { settings: { ui: { saved: {}, lib_synced: 'player', ...ui } }, account: { signedIn: true, user: { uid: 'player' } }, library: {}, owned: { found: true }, view: 'library' };
  const c = vm.createContext({
    S, byId: (id) => ({ id, title: id }), index: () => ({ titles: new Map() }), forOS: () => true,
    saveUi: async (patch) => Object.assign(S.settings.ui, patch),
    setInterval: () => {}, render: () => {}, toast: () => {}, tf: (text) => text,
    invoke: async (command, args) => {
      if (command === 'library_pull') return { saved: Object.entries(remote).map(([id, at]) => ({ id, at })) };
      assert.equal(command, 'library_push');
      for (const id of args.games) { if (args.on) remote[id] = 100; else delete remote[id]; }
      return {};
    },
  });
  vm.runInContext(library + '\n' + addOwned, c);
  return { c, S, remote };
}

test('an account refresh keeps games automatically added by a successful local scan', async () => {
  const { c, S } = fixture({}, { remote: 20 });
  assert.equal(await c.addOwned(), 1);
  await c.libSync();
  assert.ok(S.settings.ui.saved.found);
  assert.equal(S.settings.ui.saved.remote, 20);
  S.owned = {};
  await c.addOwned();
  assert.equal(S.settings.ui.saved.found, undefined, 'automatic entries still follow the local scan');
  assert.equal(S.settings.ui.saved.remote, 20);
});

test('the first account merge sends manual choices and keeps automatic discoveries local', async () => {
  const { c, S, remote } = fixture({ saved: { manual: 10 }, lib_synced: '' });
  await c.addOwned();
  await c.libSync();
  assert.ok(remote.manual);
  assert.equal(remote.found, undefined);
  assert.ok(S.settings.ui.saved.found);
  assert.equal(S.settings.ui.saved.manual, 10);
});

test('a remote library choice stops being removable by the local scan', async () => {
  const { c, S } = fixture({}, { found: 20 });
  await c.addOwned();
  await c.libSync();
  assert.equal(S.settings.ui.found_saved.includes('found'), false);
  S.owned = {};
  await c.addOwned();
  assert.equal(S.settings.ui.saved.found, 20);
});

test('a scan completed while the library request waits is retained', async () => {
  const { c, S } = fixture();
  let answer;
  c.invoke = () => new Promise((resolve) => { answer = resolve; });
  const sync = c.libSync();
  await c.addOwned();
  answer({ saved: [] });
  await sync;
  assert.ok(S.settings.ui.saved.found);
});

test('an explicitly removed automatic entry is not resurrected by an account refresh', async () => {
  const { c, S } = fixture();
  await c.addOwned();
  await c.removeFromLibrary('found');
  await c.libSync();
  await c.addOwned();
  assert.equal(S.settings.ui.saved.found, undefined);
  assert.ok(S.settings.ui.found_skip.includes('found'));
});

test('explicit remote removals still apply to manually saved games', async () => {
  const { c, S } = fixture({ saved: { manual: 10 } });
  await c.libSync();
  assert.equal(S.settings.ui.saved.manual, undefined);
});
