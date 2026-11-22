const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// rebuildAfterUpdate with what it reads stubbed: the runs it starts are recorded.
function harness(ui, su, due) {
  const runs = [];
  const S = { settings: { ui }, setup: { hopper: su } };
  const body = `${fn(app, 'rebuildAfterUpdate')}; return rebuildAfterUpdate;`;
  const make = new Function('S', 'loadSetup', 'updateBuildDue', 'prepBlocked', 'newPrepOpts', 'invoke', 'toast', 'tf', 'cleanMsg', 'byId', body);
  const run = make(S, async () => {}, () => due, () => false, (x) => (x.prepare_options || []).filter((o) => o.new), (cmd, args) => { runs.push([cmd, args]); return Promise.resolve(); }, () => {}, (x) => x, (x) => x, () => null);
  return { run, runs };
}

test('builds again after an update only when the setting is on', async () => {
  const su = { copy: '/games/hopper.iso', inputs: [] };
  const off = harness({}, su, true);
  await off.run('hopper');
  assert.deepEqual(off.runs, []);
  const on = harness({ rebuild_after_update: true }, su, true);
  await on.run('hopper');
  assert.deepEqual(on.runs, [['setup_copy', { id: 'hopper', path: '/games/hopper.iso' }]]);
});

test('never asks anything: no build without the files, or with a new option to answer', async () => {
  const ui = { rebuild_after_update: true };
  const gone = harness(ui, { copy: null, kept: false, inputs: [] }, true);
  await gone.run('hopper');
  assert.deepEqual(gone.runs, []);
  const asked = harness(ui, { copy: '/games/hopper.iso', inputs: [], prepare_options: [{ id: 'fsr', new: true }] }, true);
  await asked.run('hopper');
  assert.deepEqual(asked.runs, []);
  // Not due a build: nothing to do.
  const current = harness(ui, { copy: '/games/hopper.iso', inputs: [] }, false);
  await current.run('hopper');
  assert.deepEqual(current.runs, []);
  // Built from what it kept: no copy needed.
  const kept = harness(ui, { copy: null, kept: true, inputs: [] }, true);
  await kept.run('hopper');
  assert.deepEqual(kept.runs, [['setup_copy', { id: 'hopper', path: '' }]]);
});

test('the setting is in Settings, Updates, and off unless turned on', () => {
  assert.match(app, /sw\('rebuild_after_update', ui\.rebuild_after_update, t\('Build Games Again After Updates'\), 'ui-toggle'\)/);
  assert.match(app, /else if \(S\.easy\.has\(id\) && wasInstalled\) \{ toast\(tf\('\{name\} is updated\.', \{ name \}\)\); rebuildAfterUpdate\(id\); \}/);
});
