const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
function context(response = {}) {
  const ctx = vm.createContext({ S: { rels: {}, view: 'home' }, t: s => s, tn: (n, one, many) => n === 1 ? one : many,
    invoke: async () => response, render: () => {}, window: {}, document: { body: { classList: { contains: () => false } } } });
  for (const name of ['projectState', 'loadRels']) vm.runInContext(source.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0], ctx);
  return ctx;
}
test('abandoned projects display the existing ended warning and supplied note', () => {
  const ctx = context();
  ctx.entry = { status: 'abandoned', status_note: 'No longer maintained.' };
  assert.equal(vm.runInContext('projectState(entry)?.kind', ctx), 'ended');
  assert.equal(vm.runInContext('projectState(entry)?.note', ctx), 'No longer maintained.');
});
test('release fallback status reaches the shared desktop and Big Picture status helper', async () => {
  const ctx = context({ data: [{ tag_name: 'v1' }], archived: true });
  await vm.runInContext("loadRels('owner/game')", ctx);
  assert.equal(vm.runInContext("projectState({repo:'owner/game'})?.kind", ctx), 'ended');
  assert.equal(vm.runInContext("projectState({repo:'other/game'})", ctx), null);
  assert.equal(vm.runInContext("projectState({repo:'owner/game', archived:false})", ctx), null);
  assert.equal(vm.runInContext("projectState({repo:'owner/game', status:'complete'})?.kind", ctx), 'complete');
  assert.equal(vm.runInContext("projectState({repo:'owner/game', soon:true})", ctx), null);
});
test('unknown or false repository status does not imply abandonment', async () => {
  for (const archived of [false, undefined, 'true']) {
    const ctx = context({ data: [], archived });
    await vm.runInContext("loadRels('owner/game')", ctx);
    assert.equal(vm.runInContext("projectState({repo:'owner/game'})", ctx), null);
  }
});
test('fresh GitHub status replaces cached fallback status in either direction', async () => {
  for (const previous of [true, false]) {
    const ctx = context({ data: [], archived: !previous });
    ctx.entry = { repo: 'owner/game', github_archived: previous };
    assert.equal(vm.runInContext('projectState(entry)?.kind ?? null', ctx), previous ? 'ended' : null);
    await vm.runInContext("loadRels('owner/game')", ctx);
    assert.equal(vm.runInContext('projectState(entry)?.kind ?? null', ctx), previous ? null : 'ended');
  }
});
test('arriving repository status refreshes desktop card lists only when it changes', async () => {
  for (const view of ['home', 'discover', 'library']) {
    const response = { data: [], archived: true };
    const ctx = context(response);
    ctx.S.view = view;
    let refreshes = 0;
    ctx.render = () => { refreshes++; };
    await vm.runInContext("loadRels('owner/game')", ctx);
    assert.equal(refreshes, 1, `${view} should show the new warning`);
    await vm.runInContext("loadRels('owner/game', true)", ctx);
    assert.equal(refreshes, 1, 'unchanged status should not redraw');
    response.archived = false;
    await vm.runInContext("loadRels('owner/game', true)", ctx);
    assert.equal(refreshes, 2, `${view} should remove an outdated warning`);
  }
});
