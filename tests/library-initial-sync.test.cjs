const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const library = source.slice(source.indexOf('const saved ='), source.indexOf('const setupDone ='));

function fixture() {
  const S = { settings: { ui: { saved: { manual: 10 }, lib_synced: '', lib_queue: [] } }, account: { signedIn: true, user: { uid: 'player' } }, library: {}, view: 'library' };
  const server = { saved: {}, writes: [], fail: false };
  const c = vm.createContext({
    S, setInterval: () => {}, render: () => {}, byId: () => null,
    saveUi: async (patch) => Object.assign(S.settings.ui, patch),
    invoke: async (command, args) => {
      if (command === 'library_pull') return { saved: Object.entries(server.saved).map(([id, at]) => ({ id, at })) };
      assert.equal(command, 'library_push');
      server.writes.push([Array.from(args.games), args.on]);
      if (server.fail) throw new Error('offline');
      return {};
    },
  });
  vm.runInContext(library, c);
  return { c, S, server };
}

test('the first account upload stays queued until the website list includes it', async () => {
  const { c, S, server } = fixture();
  await c.libSync();
  assert.equal(S.settings.ui.saved.manual, 10);
  await c.libSync();
  assert.equal(S.settings.ui.saved.manual, 10, 'an unconfirmed first upload must survive later refreshes');
  assert.deepEqual(server.writes, [[['manual'], true], [['manual'], true]]);
});

test('a website acknowledgement clears the initial queue and later remote removal applies', async () => {
  const { c, S, server } = fixture();
  await c.libSync();
  assert.equal(S.settings.ui.lib_queue.length, 1);
  server.saved.manual = 20;
  await c.libSync();
  assert.equal(S.settings.ui.lib_queue.length, 0);
  assert.equal(S.settings.ui.saved.manual, 20);
  delete server.saved.manual;
  await c.libSync();
  assert.equal(S.settings.ui.saved.manual, undefined);
});

test('removing an unconfirmed first upload cancels the queued addition', async () => {
  const { c, S } = fixture();
  await c.libSync();
  assert.equal(S.settings.ui.lib_queue.length, 1);
  await c.removeFromLibrary('manual');
  await c.libSync();
  assert.equal(S.settings.ui.saved.manual, undefined);
  assert.equal(S.settings.ui.lib_queue.length, 0);
});

test('a failed first upload keeps the choice queued for the next sync', async () => {
  const { c, S, server } = fixture();
  server.fail = true;
  await c.libSync();
  assert.equal(S.settings.ui.saved.manual, 10);
  assert.equal(S.settings.ui.lib_queue.length, 1);
  server.fail = false;
  server.saved.manual = 20;
  await c.libSync();
  assert.ok(S.settings.ui.saved.manual);
  assert.equal(S.settings.ui.lib_queue.length, 0);
});
