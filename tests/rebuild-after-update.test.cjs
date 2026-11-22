const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js');

// The builds-after-an-update block of app.js, with what it reads stubbed: the runs it starts, the
// toasts it shows and the windows it opens are recorded.
function harness(library, setup, due) {
  const block = app.slice(app.indexOf('// Builds after an update:'), app.indexOf('async function toggleRebuildAfterUpdate'));
  const log = { runs: [], toasts: [], windows: [] };
  const S = { library, setup, setupJobs: {}, running: new Set() };
  const SU = { stage: 'steps' };
  const names = ['S', 'SU', 'loadSetup', 'updateBuildDue', 'prepBlocked', 'newPrepOpts', 'invoke', 'toast', 'tf', 't', 'esc', 'cleanMsg', 'byId', 'render', 'updateBuildModal'];
  const make = new Function(...names, `${block}; return { queueBuild, pumpBuilds, afterUpdate, buildWaits };`);
  const api = make(S, SU, async () => {}, (id) => due.has(id), () => false, (su) => (su.prepare_options || []).filter((o) => o.new),
    (cmd, args) => { log.runs.push([cmd, args.id]); return Promise.resolve(); }, (msg, bad, html) => log.toasts.push(html || msg),
    (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]), (x) => x, (x) => x, (x) => x, () => null, () => {}, (id) => log.windows.push(id));
  return { api, log, S };
}
const copy = (path) => ({ copy: path, inputs: [] });

test('a game with Build Again After Updates on builds right after its update', async () => {
  const { api, log } = harness({ hopper: { rebuild_after_update: true } }, { hopper: copy('/g/hopper') }, new Set(['hopper']));
  await api.afterUpdate('hopper', 'Hopper');
  assert.deepEqual(log.runs, [['setup_copy', 'hopper']]);
});

test('with it off, nothing builds now: Play builds it, and the toast offers nothing', async () => {
  const { api, log } = harness({ hopper: {} }, { hopper: copy('/g/hopper') }, new Set(['hopper']));
  await api.afterUpdate('hopper', 'Hopper');
  assert.deepEqual(log.runs, []);
  assert.deepEqual(log.toasts, ['Hopper is updated.']);
});

test('a game not due a build after its update is only said to be updated', async () => {
  const { api, log } = harness({ hopper: { rebuild_after_update: true } }, { hopper: copy('/g/hopper') }, new Set());
  await api.afterUpdate('hopper', 'Hopper');
  assert.deepEqual(log.runs, []);
  assert.deepEqual(log.toasts, ['Hopper is updated.']);
});

test('several builds wait in a queue and run one at a time', async () => {
  const lib = { a: { rebuild_after_update: true }, b: { rebuild_after_update: true } };
  const { api, log, S } = harness(lib, { a: copy('/g/a'), b: copy('/g/b') }, new Set(['a', 'b']));
  await api.afterUpdate('a', 'A');
  await api.afterUpdate('b', 'B');
  assert.deepEqual(log.runs, [['setup_copy', 'a']]);
  assert.ok(api.buildWaits('b'));
  // A's build ended (the setup event clears it): B goes next.
  S.building = null;
  await api.pumpBuilds();
  assert.deepEqual(log.runs, [['setup_copy', 'a'], ['setup_copy', 'b']]);
});

test('a queued build waits while another setup runs', async () => {
  const { api, log, S } = harness({ a: {} }, { a: copy('/g/a') }, new Set(['a']));
  S.setupJobs.other = { step: 'extras' };
  api.queueBuild('a');
  await api.pumpBuilds();
  assert.deepEqual(log.runs, []);
  delete S.setupJobs.other;
  await api.pumpBuilds();
  assert.deepEqual(log.runs, [['setup_copy', 'a']]);
});

test('with it on, a game that has to ask something first waits for Play', async () => {
  const asks = { copy: '/g/a', inputs: [], prepare_options: [{ id: 'fsr', new: true }] };
  const { api, log } = harness({ a: { rebuild_after_update: true } }, { a: asks }, new Set(['a']));
  await api.afterUpdate('a', 'A');
  assert.deepEqual(log.runs, []);
  assert.deepEqual(log.windows, []);
  const gone = harness({ b: { rebuild_after_update: true } }, { b: { copy: null, kept: false, inputs: [] } }, new Set(['b']));
  await gone.api.afterUpdate('b', 'B');
  assert.deepEqual(gone.log.runs, []);
});

test('the switch is per game, in its Manage tab, and Settings has none', () => {
  assert.match(app, /data-act="rebuild-up" data-id="\$\{esc\(id\)\}"/);
  assert.match(app, /aria-checked="\$\{!!inst\.rebuild_after_update\}"/);
  assert.doesNotMatch(app, /sw\('rebuild_after_update'/);
  assert.match(app, /else if \(S\.easy\.has\(id\) && wasInstalled\) afterUpdate\(id, name\);/);
  assert.doesNotMatch(app, /build-now|buildNow/);
});
