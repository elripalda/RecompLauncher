const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// One game with updates paused: the check finds v2 and keeps it apart until Resume Updates.
async function paused() {
  const starts = [];
  const S = {
    library: { held: { repo: 'Studio/Game', path: 'C:/Games/Game', tag: 'v1', installed_at: 10, updated_at: 10 } },
    settings: { ui: { hold_updates: ['held'] } }, running: new Set(), jobs: {}, paused: {}, dlArgs: {}, updates: {}, view: 'library',
  };
  const c = vm.createContext({
    S, updateCheckRevision: 0, invoke: async () => ({ held: 'v2' }), window: {},
    document: { body: { classList: { contains: () => false } } },
    saveUi: async (part) => { S.settings.ui = { ...S.settings.ui, ...part }; },
    renderSide() {}, render() {}, toast() {}, t: (s) => s, tf: (s) => s,
    updatingLine: () => '', readyLine: () => '', badVersion: () => false,
    byId: (id) => ({ id }), autoUpdates: () => true,
    updateGame: (id) => { starts.push(id); }, fail: (error) => { throw error; },
  });
  const code = ['checkUpdates', 'stillChecked', 'holdUpdates', 'heldIds', 'toggleHold'].map(fn).join('\n');
  vm.runInContext(code + '\n' + app.match(/^function dlArgs\(.*$/m)[0], c);
  await c.checkUpdates();
  return { S, c, starts };
}

test('a paused update is kept apart and not installed', async () => {
  const p = await paused();
  assert.equal(p.S.held.held, 'v2');
  assert.equal(p.S.updates.held, undefined);
  assert.deepEqual(p.starts, []);
});

test('resuming updates with nothing changed offers the paused update', async () => {
  const p = await paused();
  await p.c.toggleHold('held');
  assert.equal(p.S.updates.held, 'v2');
  assert.equal(p.S.held.held, undefined);
});

test('resuming after a newer version was installed drops the paused update', async () => {
  const p = await paused();
  p.S.library.held = { ...p.S.library.held, tag: 'v3', updated_at: 20 };
  await p.c.toggleHold('held');
  assert.equal(p.S.updates.held, undefined);
  assert.equal(p.S.held.held, undefined);
});

test('resuming after the player picked an older release drops the paused update', async () => {
  const p = await paused();
  p.S.library.held = { ...p.S.library.held, tag: 'v0', pinned: true, updated_at: 20 };
  await p.c.toggleHold('held');
  assert.equal(p.S.updates.held, undefined);
});

test('resuming while a picked release downloads drops the paused update', async () => {
  const p = await paused();
  p.c.dlArgs('held', { id: 'held', dir: null, tag: 'v0' });
  await p.c.toggleHold('held');
  assert.equal(p.S.updates.held, undefined);
});

test('the next check after resuming does not install the dropped update', async () => {
  const p = await paused();
  p.S.library.held = { ...p.S.library.held, tag: 'v0', pinned: true, updated_at: 20 };
  await p.c.toggleHold('held');
  await p.c.checkUpdates();
  assert.deepEqual(p.starts, []);
  assert.equal(p.S.updates.held, undefined);
});
