const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const test = require('node:test');
const { runInNewContext } = require('node:vm');

// Cloud saves are picked once for every game (account.js askCloudAll), not asked game by game
// before Play. A game switched in its Manage tab keeps that.
const account = readFileSync(join(__dirname, '../ui/account.js'), 'utf8');
const app = readFileSync(join(__dirname, '../ui/app.js'), 'utf8');
const between = (src, from, to) => src.slice(src.indexOf(from), src.indexOf(to));
const cloudOnSrc = app.match(/^const cloudOn = .*$/m)[0];
const askSrc = between(account, 'async function askCloudAll(', 'window.askCloudAll = askCloudAll;');
const playSrc = between(account, 'async function beforeCloudPlay(', 'async function pickCloudSave(');

function cloudOn(ui) {
  return runInNewContext(cloudOnSrc + '\ncloudOn', { S: { settings: { ui } } });
}

function asker({ ui = {}, signedIn = true, setupDone = true, firstRun = false, answer = true } = {}) {
  const calls = [];
  const S = { settings: { ui, setup_done: setupDone }, account: { signedIn }, view: 'home' };
  const ask = runInNewContext(askSrc + '\naskCloudAll', {
    S,
    cloudAsked: false,
    document: { body: { classList: { contains: (c) => c === 'first-run' && firstRun } } },
    noModal: async () => {},
    pickCloudAll: async () => { calls.push('asked'); return answer; },
    setCloudAll: async (v) => { calls.push(['set', v]); },
    toast() {},
    t: String,
    render() {},
  });
  return { calls, ask };
}

test('a game never switched follows the pick for every game', () => {
  assert.equal(cloudOn({})({ cloud: null }), false);
  assert.equal(cloudOn({ cloud_all: true })({ cloud: null }), true);
  assert.equal(cloudOn({ cloud_all: false })({ cloud: null }), false);
});

test('a game switched in its Manage tab keeps that', () => {
  assert.equal(cloudOn({ cloud_all: true })({ cloud: false }), false);
  assert.equal(cloudOn({ cloud_all: false })({ cloud: true }), true);
});

test('signed-in players are asked once and their answer is kept', async () => {
  const { calls, ask } = asker({ answer: false });
  await ask();
  await ask();
  assert.deepEqual(calls, ['asked', ['set', false]]);
});

test('closing the question without an answer keeps nothing', async () => {
  const { calls, ask } = asker({ answer: null });
  await ask();
  assert.deepEqual(calls, ['asked']);
});

test('nobody is asked when signed out, during setup, or after answering', async () => {
  for (const o of [{ signedIn: false }, { setupDone: false }, { firstRun: true }, { ui: { cloud_all: true } }]) {
    const { calls, ask } = asker(o);
    await ask();
    assert.deepEqual(calls, []);
  }
});

test('Don\'t Ask Again to the old question means only the games they pick', async () => {
  const { calls, ask } = asker({ ui: { cloud_ask: false } });
  await ask();
  assert.deepEqual(calls, [['set', false]]);
});

test('Play asks nothing about cloud saves for a game never switched', async () => {
  const invoked = [];
  const S = { library: { g: { name: 'Game', cloud: null } }, account: { signedIn: true }, settings: { ui: {} } };
  const context = { S, invoke: async (c) => { invoked.push(c); return { action: 'play' }; }, toast() {}, savesChanged() {}, fail: assert.fail, t: String, tf: String };
  context.cloudOn = runInNewContext(cloudOnSrc + '\ncloudOn', context);
  const play = runInNewContext(playSrc + '\nbeforeCloudPlay', context);
  assert.equal(await play('g'), true);
  assert.deepEqual(invoked, []);
  S.settings.ui.cloud_all = true;
  assert.equal(await play('g'), true);
  assert.deepEqual(invoked, ['saves_check']);
});
