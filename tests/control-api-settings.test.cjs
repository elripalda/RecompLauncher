const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

// The Local control switch: desktop Settings and Big Picture mode both go through the same
// setControlApi, which asks the core to start or stop the local control API.
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');

function fn(name) {
  const match = app.match(new RegExp(`^(async )?function ${name}\\([^]*?^}`, 'm'));
  assert.ok(match, `${name} is in app.js`);
  return match[0];
}

function context(settings, answer, now = null) {
  const calls = [], toasts = [];
  const ctx = vm.createContext({
    S: { settings, controlApi: null },
    window: {},
    calls,
    toasts,
    invoke: async (cmd, args) => { calls.push([cmd, args]); if (answer instanceof Error) throw answer; return answer; },
    // get_state, as refreshState asks for it: the core's setting and state now.
    refreshState: async () => { calls.push(['get_state']); if (now) Object.assign(ctx.S, now); },
    toast: (m) => toasts.push(m),
    fail: (e) => toasts.push('fail: ' + e.message),
    t: (s) => s,
    tf: (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]),
  });
  vm.runInContext(fn('setControlApi') + '\n' + fn('controlApiNote'), ctx);
  return ctx;
}

test('turning it on asks the core and shows where the endpoint file is', async () => {
  const ctx = context({}, { on: true, running: true, endpoint: '/data/control-api/endpoint.json', error: null });
  await vm.runInContext('setControlApi(true)', ctx);
  assert.equal(JSON.stringify(ctx.calls), JSON.stringify([['control_api_set', { on: true }]]));
  assert.equal(ctx.S.settings.control_api, true);
  assert.match(vm.runInContext('controlApiNote()', ctx), /\/data\/control-api\/endpoint\.json/);
  assert.equal(ctx.toasts.length, 0);
});

test('when it cannot turn on, the switch stays off and says why', async () => {
  const ctx = context({}, { on: false, running: false, endpoint: null, error: 'this drive doesn\'t keep access lists' });
  await vm.runInContext('setControlApi(true)', ctx);
  assert.equal(ctx.S.settings.control_api, false);
  assert.equal(ctx.toasts.length, 1);
  assert.match(ctx.toasts[0], /access lists/);
});

test('turning it off asks the core to stop it', async () => {
  const ctx = context({ control_api: true }, { on: false, running: false, endpoint: null, error: null });
  await vm.runInContext('setControlApi(false)', ctx);
  assert.equal(JSON.stringify(ctx.calls), JSON.stringify([['control_api_set', { on: false }]]));
  assert.equal(ctx.S.settings.control_api, false);
  assert.equal(ctx.toasts.length, 0);
});

test('turned on but not saved, it is left off and says so', async () => {
  const ctx = context({}, { on: false, running: false, endpoint: null, error: null, unsaved: 'No space left on device' });
  await vm.runInContext('setControlApi(true)', ctx);
  assert.equal(ctx.S.settings.control_api, false);
  assert.equal(ctx.toasts.length, 1);
  assert.match(ctx.toasts[0], /off: the setting couldn't be saved \(No space left on device\)/);
});

test('turned off but not saved, it is off and warns it may come back on', async () => {
  const ctx = context({ control_api: true }, { on: false, running: false, endpoint: null, error: null, unsaved: 'Permission denied' });
  await vm.runInContext('setControlApi(false)', ctx);
  assert.equal(ctx.S.settings.control_api, false);
  assert.equal(ctx.toasts.length, 1);
  assert.match(ctx.toasts[0], /off, but that couldn't be saved \(Permission denied\)\. It may turn on again/);
});

test('when the call itself fails, the switch takes what the core says now', async () => {
  const ctx = context({ control_api: false }, new Error('lost'), { settings: { control_api: true }, controlApi: { running: true, endpoint: '/e.json' } });
  await vm.runInContext('setControlApi(true)', ctx);
  assert.equal(JSON.stringify(ctx.calls), JSON.stringify([['control_api_set', { on: true }], ['get_state']]));
  assert.equal(ctx.S.settings.control_api, true);
  assert.match(ctx.toasts[0], /fail: lost/);
});

test('off by default, the note says what it would allow and what it never does', () => {
  const ctx = context({}, null);
  const note = vm.runInContext('controlApiNote()', ctx);
  assert.match(note, /see your games and start one/);
  assert.match(note, /installing, removing and settings stay here/);
});

test('turned off with a Play still finishing getting ready, the note says it won\'t start its game', async () => {
  const ctx = context({ control_api: true }, { on: false, running: false, endpoint: null, error: null, stopping: true });
  await vm.runInContext('setControlApi(false)', ctx);
  assert.equal(ctx.S.settings.control_api, false);
  assert.match(vm.runInContext('controlApiNote()', ctx), /still finishing that; it won't start its game/);
});

test('desktop Settings and Big Picture mode both show the switch and use setControlApi', () => {
  // Desktop: a switch on the saved setting (unset is off), handled by its own action.
  assert.match(app, /sw\('control_api', st\.control_api, t\('Local control'\), 'control-api'\)/);
  assert.match(app, /case 'control-api': \{\s*await setControlApi\(!S\.settings\.control_api\);/);
  // Big Picture: the same setting, the same call.
  assert.match(tv, /data-tvact="control-api" data-sw="\$\{S\.settings\.control_api \? 1 : 0\}"/);
  assert.match(tv, /case 'control-api': sfx\('select'\); await setControlApi\(!S\.settings\.control_api\);/);
});

test('the generic settings save never carries the switch to the core', () => {
  // save_settings keeps whatever the switch last set (see lib.rs), so only control_api_set
  // starts or stops the API. The toggle action for other switches doesn't handle it.
  assert.doesNotMatch(app, /saveSettings\(\{ control_api/);
  assert.doesNotMatch(tv, /data-toggle="control_api"/);
});
