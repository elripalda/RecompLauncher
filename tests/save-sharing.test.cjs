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
