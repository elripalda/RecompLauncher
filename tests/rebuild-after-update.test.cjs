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

// The Downloads page's view of builds: what a game is doing (jobOf) and the words it shows.
const topFn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];
const topConst = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];
function downloadsView(S) {
  const src = [topConst('pct'), topConst('buildWaits'), topFn('buildJob'), topConst('jobOf'), topFn('progLabel'), topFn('dlPhase')].join('\n');
  const make = new Function('S', 't', 'tf', `${src}; return { jobOf, progLabel, dlPhase };`);
  return make(S, (x) => x, (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]));
}

test('the game building shows the builder\'s line and how far it is', () => {
  const S = { jobs: {}, building: 'a', buildQ: [], setupJobs: { a: { step: 'extras', pct: 0.42, text: 'Compiling funcs_12.c' } } };
  const v = downloadsView(S);
  assert.equal(v.dlPhase('a'), 'Building the game · 42%');
  assert.deepEqual(v.progLabel(v.jobOf('a')), ['Compiling funcs_12.c', '42%']);
});

test('a build waiting its turn says so', () => {
  const v = downloadsView({ jobs: {}, building: 'a', buildQ: ['b'], setupJobs: {} });
  assert.equal(v.dlPhase('b'), 'Waiting to build');
  assert.deepEqual(v.progLabel(v.jobOf('b')), ['Waiting for the build before it', '']);
});

test('a game downloading again shows its download, not its build', () => {
  const download = { id: 'a', phase: 'download', done: 5, total: 10 };
  const v = downloadsView({ jobs: { a: download }, building: null, buildQ: ['a'], setupJobs: {} });
  assert.equal(v.jobOf('a'), download);
});

test('builds are jobs on the Downloads page, on the desktop and in Big Picture', () => {
  const tv = read('ui/tv.js');
  assert.match(app, /const act = Object\.keys\(S\.jobs\)\.concat\(.*, builds\)/);
  assert.match(tv, /const act = Object\.keys\(S\.jobs\)\.concat\(builds\)/);
});

test('a build that didn\'t finish offers Try again in the Finish the Update window', () => {
  assert.match(app, /if \(r\.build\) return .*data-act="build-again"/);
  assert.match(app, /case 'build-again': .*return updateBuildModal\(id\);/);
});

test('cancelling the build clears its progress, so the next one can start', () => {
  assert.match(app, /if \(S\.building === id\) \{ S\.building = null; delete S\.setupJobs\[id\];/);
});

test('the switch is per game, in its Manage tab, and Settings has none', () => {
  assert.match(app, /data-act="rebuild-up" data-id="\$\{esc\(id\)\}"/);
  assert.match(app, /aria-checked="\$\{!!inst\.rebuild_after_update\}"/);
  assert.doesNotMatch(app, /sw\('rebuild_after_update'/);
  assert.match(app, /else if \(S\.easy\.has\(id\) && wasInstalled\) afterUpdate\(id, name\);/);
  assert.doesNotMatch(app, /build-now|buildNow/);
});
