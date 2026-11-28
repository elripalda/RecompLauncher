const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const topFn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// Skip This Version and the update check that reads it, with a game its own updater already
// moved to v2 while the launcher still offers v2.
function context(found) {
  const game = { id: 'tsfp', name: 'TimeSplitters Future Perfect', repo: 'a/b', path: '/g', tag: 'v1', installed_at: 1, updated_at: 1 };
  const c = vm.createContext({
    S: { library: { tsfp: game }, updates: { tsfp: 'v2' }, settings: { ui: {} }, recent: [], running: new Set(), jobs: {}, uninstalling: new Set(), view: 'downloads' },
    window: {},
    document: { body: { classList: { contains: () => false } } },
    updateCheckRevision: 0,
    toasts: [],
    found,
  });
  vm.runInContext(`
    const t = (s) => s, tf = (s, v) => s.replace(/\\{(\\w+)\\}/g, (_, k) => v[k]);
    const toast = (m) => toasts.push(m), renderSide = () => {}, render = () => {}, fail = (e) => { throw e; };
    const saveUi = async (patch) => { S.settings.ui = Object.assign({}, S.settings.ui, patch); };
    const invoke = async (cmd) => (cmd === 'check_updates' ? found() : null);
    const autoUpdates = () => false, parentId = (id) => id, badVersion = () => false, updateGame = () => {};
    const readyLine = () => '', updatingLine = () => '';
    ${topFn('heldIds')}${topFn('holdUpdates')}${topFn('stillChecked')}${topFn('skipVersion')}${topFn('checkUpdates')}
    globalThis.skipVersion = skipVersion; globalThis.checkUpdates = checkUpdates;`, c);
  return c;
}

test('Skip This Version takes the update off the list and says so', async () => {
  const c = context(() => ({ tsfp: 'v2' }));
  await c.skipVersion('tsfp');
  assert.equal(c.S.updates.tsfp, undefined);
  assert.equal(c.S.settings.ui.skip_ver.tsfp, 'v2');
  assert.match(c.toasts[0], /TimeSplitters Future Perfect stays on the version it has/);
});

test('a skipped version stays out of the next update check', async () => {
  const c = context(() => ({ tsfp: 'v2' }));
  await c.skipVersion('tsfp');
  await c.checkUpdates(false);
  assert.equal(c.S.updates.tsfp, undefined);
});

test('the version after the skipped one is offered again', async () => {
  let out = { tsfp: 'v2' };
  const c = context(() => out);
  await c.skipVersion('tsfp');
  out = { tsfp: 'v3' };
  await c.checkUpdates(false);
  assert.equal(c.S.updates.tsfp, 'v3');
});

test('Skip This Version does nothing for a game with no update on offer', async () => {
  const c = context(() => ({}));
  delete c.S.updates.tsfp;
  await c.skipVersion('tsfp');
  assert.equal(c.S.settings.ui.skip_ver, undefined);
  assert.equal(c.toasts.length, 0);
});
