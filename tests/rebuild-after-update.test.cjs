const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js');

// The builds-after-an-update block of app.js (and updateGame, which starts updates), with what it
// reads stubbed: the runs and downloads it starts, the toasts it shows and the windows it opens
// are recorded. A download started shows as a job until the test says it's in (landed).
function harness(library, setup, due) {
  const block = app.slice(app.indexOf('// Builds after an update:'), app.indexOf('async function toggleRebuildAfterUpdate'));
  const updateGame = app.match(/^function updateGame\([^]*?^}/m)[0];
  const log = { runs: [], toasts: [], windows: [], downloads: [] };
  const S = { library, setup, setupJobs: {}, running: new Set(), jobs: {}, updates: {} };
  const SU = { stage: 'steps' };
  const startInstall = (id) => { log.downloads.push(id); S.jobs[id] = { id, phase: 'download' }; return Promise.resolve(); };
  const names = ['S', 'SU', 'loadSetup', 'updateBuildDue', 'prepBlocked', 'newPrepOpts', 'invoke', 'toast', 'tf', 't', 'esc', 'cleanMsg', 'byId', 'render', 'updateBuildModal', 'parentId', 'startInstall', 'window'];
  const make = new Function(...names, `${block}\n${updateGame}; return { queueBuild, pumpBuilds, afterUpdate, buildWaits, updateGame, nextInLine, cancelDownload };`);
  const api = make(S, SU, async () => {}, (id) => due.has(id), () => false, (su) => (su.prepare_options || []).filter((o) => o.new),
    (cmd, args) => { log.runs.push([cmd, args.id]); return Promise.resolve(); }, (msg, bad, html) => log.toasts.push(html || msg),
    (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]), (x) => x, (x) => x, (x) => x, () => null, () => {}, (id) => log.windows.push(id),
    (id) => id, startInstall, {});
  // Its download is in: as the install event does, the job goes and afterUpdate runs.
  api.landed = async (id) => { delete S.jobs[id]; await api.afterUpdate(id, id); };
  // Its build is over: as the setup event does.
  api.built = (id) => { S.building = null; delete S.setupJobs[id]; api.nextInLine(); };
  return { api, log, S };
}
const settle = () => new Promise((ok) => setTimeout(ok, 0));
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

test('two updates landing at once still build one at a time', async () => {
  const lib = { a: { rebuild_after_update: true }, b: { rebuild_after_update: true } };
  const { api, log } = harness(lib, { a: copy('/g/a'), b: copy('/g/b') }, new Set(['a', 'b']));
  // Not awaited one by one: the second lands while the first is still reading its game.
  await Promise.all([api.afterUpdate('a', 'A'), api.afterUpdate('b', 'B')]);
  await new Promise((ok) => setTimeout(ok, 0));
  assert.deepEqual(log.runs, [['setup_copy', 'a']]);
  assert.ok(api.buildWaits('b'));
});

test('the next update that builds waits, download and all, for the build before it', async () => {
  const lib = { a: { rebuild_after_update: true }, b: { rebuild_after_update: true } };
  const { api, log, S } = harness(lib, { a: copy('/g/a'), b: copy('/g/b') }, new Set(['a', 'b']));
  api.updateGame('a');
  api.updateGame('b');
  await settle();
  assert.deepEqual(log.downloads, ['a']);
  assert.equal(S.jobs.b.held, true);
  await api.landed('a');
  await settle();
  assert.deepEqual(log.runs, [['setup_copy', 'a']]);
  assert.deepEqual(log.downloads, ['a']);
  api.built('a');
  await settle();
  assert.deepEqual(log.downloads, ['a', 'b']);
});

test('a game that builds on Play downloads its update at once', async () => {
  const lib = { a: { rebuild_after_update: true }, c: {} };
  const { api, log } = harness(lib, { a: copy('/g/a'), c: copy('/g/c') }, new Set(['a', 'c']));
  api.updateGame('a');
  api.updateGame('c');
  await settle();
  assert.deepEqual(log.downloads, ['a', 'c']);
});

test('the next update goes as soon as the one before turns out to need the player', async () => {
  const lib = { a: { rebuild_after_update: true }, b: { rebuild_after_update: true } };
  const gone = { copy: null, kept: false, inputs: [] };
  const { api, log } = harness(lib, { a: gone, b: copy('/g/b') }, new Set(['a', 'b']));
  api.updateGame('a');
  api.updateGame('b');
  await settle();
  await api.landed('a');
  await settle();
  assert.deepEqual(log.runs, []);
  assert.deepEqual(log.downloads, ['a', 'b']);
});

test('Cancel takes an update waiting its turn out of line', async () => {
  const lib = { a: { rebuild_after_update: true }, b: { rebuild_after_update: true } };
  const { api, log, S } = harness(lib, { a: copy('/g/a'), b: copy('/g/b') }, new Set(['a', 'b']));
  api.updateGame('a');
  api.updateGame('b');
  await settle();
  await api.cancelDownload('b');
  assert.equal(S.jobs.b, undefined);
  await api.landed('a');
  api.built('a');
  await settle();
  assert.deepEqual(log.downloads, ['a']);
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
