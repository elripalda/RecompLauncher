const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const test = require('node:test');
const { runInNewContext } = require('node:vm');

const account = readFileSync(join(__dirname, '../ui/account.js'), 'utf8');
const providerHelpers = account.slice(account.indexOf('S.saveProvider ='), account.indexOf('async function loadAccount('));

function providerSetup(picked) {
  const S = { library: { game: { cloud: true } }, saves: {}, savesAll: {} };
  const calls = [];
  const helpers = runInNewContext(providerHelpers + '\n({chooseSaveProvider, cloudReady})', {
    S, window: {}, t: String, tf: String, tn: String, esc: String,
    ask: async () => picked,
    invoke: async (command, args) => {
      calls.push([command, { ...args }]);
      return { state: { provider: args.provider, connected: true }, installed: { game: { cloud: null } } };
    },
    renderAccountBits() {},
  });
  return { S, calls, ...helpers };
}

test('cancelling a provider change keeps the existing game consent', async () => {
  const { chooseSaveProvider, calls, S } = providerSetup(false);
  await chooseSaveProvider('google_drive');
  assert.deepEqual(calls, []);
  assert.equal(S.library.game.cloud, true);
});

test('confirming a provider change uses the new consent returned by the backend', async () => {
  const { chooseSaveProvider, calls, S } = providerSetup(true);
  await chooseSaveProvider('google_drive');
  assert.deepEqual(calls, [['save_provider_set', { provider: 'google_drive' }]]);
  assert.equal(S.library.game.cloud, null);
  assert.equal(S.saveProvider.provider, 'google_drive');
});

test('selecting the current provider does not reset consent', async () => {
  const { chooseSaveProvider, calls, S } = providerSetup(true);
  await chooseSaveProvider('recompstore');
  assert.deepEqual(calls, []);
  assert.equal(S.library.game.cloud, true);
});

test('Drive sync before play does not need a RecompStore sign-in', async () => {
  const source = account.slice(account.indexOf('async function beforeCloudPlay('), account.indexOf('async function pickCloudSave('));
  const calls = [];
  const play = runInNewContext(source + '\nbeforeCloudPlay', {
    S: { account: { signedIn: false }, library: { game: { cloud: true } } },
    cloudReady: () => true,
    invoke: async (command, args) => { calls.push([command, { ...args }]); return { action: 'play' }; },
    savesChanged() {}, toast() {}, fail: assert.fail,
  });
  assert.equal(await play('game'), true);
  assert.deepEqual(calls, [['saves_check', { id: 'game' }]]);
});

test('Drive conflict choices submit the revision the player reviewed', async () => {
  const source = account.slice(account.indexOf('async function pickCloudSave('), account.indexOf('// ---------- actions ----------'));
  const calls = [];
  const pick = runInNewContext(source + '\npickCloudSave', {
    invoke: async (command, args) => calls.push([command, { ...args }]),
    ask: async () => 'local',
    t: String, tf: String, esc: String, whenText: String, fmtTime: String, toast() {},
  });
  await pick('game', { local: { hash: 'local', when: 100 }, cloud: { v: 'file', revision: 'both-heads', made: 200 } });
  assert.deepEqual(calls, [['saves_pick', { id: 'game', keep: 'local', localHash: 'local', cloudVersion: 'file', cloudRevision: 'both-heads' }]]);
});
