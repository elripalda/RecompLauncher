const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const test = require('node:test');
const { runInNewContext } = require('node:vm');

const account = readFileSync(join(__dirname, '../ui/account.js'), 'utf8');
const beforePlay = account.slice(account.indexOf('async function beforePlay('), account.indexOf('async function beforeCloudPlay('));

function setup(picked, enabled = true) {
  const calls = [];
  const offer = { name: 'Version A', sourceWhen: 200, destinationWhen: 100, selection: {} };
  const play = runInNewContext(beforePlay + '\nbeforePlay', {
    beforeCloudPlay: async () => true,
    saveShareTitle: () => 'game',
    saveShareOn: () => enabled,
    toggleSaveShare: async () => calls.push('off'),
    invoke: async (command, { selection }) => {
      assert.equal(command, 'saves_share');
      calls.push(selection === null ? 'check' : 'copy');
      assert.equal(selection === null || selection === offer.selection, true);
      return offer;
    },
    ask: async () => picked,
    savesChanged() {},
    fail: assert.fail,
    toast() {},
    t: String,
    tf: String,
    esc: String,
    whenText: String,
  });
  return { calls, play };
}

test('save sharing is opt-in and copies only on confirmation', async () => {
  for (const [picked, enabled, starts, expected] of [
    ['copy', false, true, []],
    ['keep', true, true, ['check']],
    ['off', true, true, ['check', 'off']],
    [null, true, false, ['check']],
    ['copy', true, true, ['check', 'copy']],
  ]) {
    const { calls, play } = setup(picked, enabled);
    assert.equal(await play('version-b'), starts);
    assert.deepEqual(calls, expected);
  }
});

test('Sync now resolves only the selected recomp and submits the copies the player saw', async () => {
  const helpers = account.slice(account.indexOf('async function pickCloudSave('), account.indexOf('// ---------- actions ----------'));
  const local = { hash: 'local-hash', when: 200, device: 'Laptop' };
  const cloud = { v: 'cloud-version', made: 100, device: 'Desktop' };
  for (const picked of ['local', 'cloud', null]) {
    const calls = [];
    const sync = runInNewContext(helpers + '\nsyncCloudNow', {
      invoke: async (command, args) => {
        calls.push([command, { ...args }]);
        return { action: 'conflict', local, cloud };
      },
      ask: async (_title, _body, buttons) => {
        assert.equal(buttons[0].sub, '200 · Laptop');
        assert.equal(buttons[1].sub, '100 · Desktop');
        return picked;
      },
      t: String, tf: String, esc: String, whenText: String, fmtTime: String, toast() {},
    });
    assert.equal(await sync('version-b'), picked !== null);
    const expected = [['saves_check', { id: 'version-b', manual: true }]];
    if (picked) expected.push(['saves_pick', { id: 'version-b', keep: picked, localHash: local.hash, cloudVersion: cloud.v }]);
    assert.deepEqual(calls, expected);
  }
});
