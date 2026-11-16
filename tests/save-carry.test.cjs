const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const test = require('node:test');
const { runInNewContext } = require('node:vm');

const account = readFileSync(join(__dirname, '../ui/account.js'), 'utf8');
const carry = account.slice(account.indexOf('function carryText('), account.indexOf('async function beforePlay('));

// A version installed but never played, and another version of the game whose saves can come over.
function setup({ picked = 'carry', played = 0, has = false } = {}) {
  const calls = [];
  const offer = { id: 'version-a', name: 'Version A', files: 1, size: 10, when: 200, has };
  const first = runInNewContext(carry + '\ncarryOnFirstPlay', {
    S: { library: { 'version-b': { last_played: played } } },
    invoke: async (command, args) => {
      calls.push(command);
      if (command === 'saves_carry') assert.equal(`${args.from} > ${args.to}`, 'version-a > version-b');
      return command === 'saves_carry_from' ? { offers: [offer], blocked: [] } : 1;
    },
    ask: async () => { calls.push('ask'); return picked; },
    savesChanged() {},
    fail: assert.fail,
    toast() {},
    t: String,
    tf: String,
    tn: String,
    esc: String,
    whenText: String,
  });
  return { calls, first };
}

test('a first start brings the saves over when the player says so', async () => {
  const { calls, first } = setup();
  assert.equal(await first('version-b'), true);
  assert.deepEqual(calls, ['saves_carry_from', 'ask', 'saves_carry']);
});

test('a first start can begin without the other version\'s saves', async () => {
  const { calls, first } = setup({ picked: 'fresh' });
  assert.equal(await first('version-b'), true);
  assert.deepEqual(calls, ['saves_carry_from', 'ask']);
});

test('closing the question does not start the game', async () => {
  const { first } = setup({ picked: null });
  assert.equal(await first('version-b'), false);
});

test('a version played before is not asked again', async () => {
  const { calls, first } = setup({ played: 100 });
  assert.equal(await first('version-b'), true);
  assert.deepEqual(calls, []);
});

test('a version with saves of its own is not asked on its first start', async () => {
  const { calls, first } = setup({ has: true });
  assert.equal(await first('version-b'), true);
  assert.deepEqual(calls, ['saves_carry_from']);
});

// Hookshot installed and played; Longshot about to be installed, or installed but not started yet.
function switching({ picked = 'switch', saves = 'maybe', found = 'offer' } = {}) {
  const calls = [];
  const toasts = [];
  const S = {
    settings: { ui: {} },
    library: { hookshot: { id: 'hookshot', name: 'Hookshot', last_played: 100 }, longshot: { id: 'longshot', name: 'Longshot' } },
  };
  const offers = found === 'offer' ? [{ id: 'hookshot', name: 'Hookshot', files: 2, when: 200, has: false }] : [];
  const blocked = found === 'blocked' ? [{ id: 'hookshot', name: 'Hookshot', why: 'Laid out differently.', empty: false }]
    : found === 'empty' ? [{ id: 'hookshot', name: 'Hookshot', why: 'No saves yet.', empty: true }] : [];
  const api = runInNewContext(carry + '\n({ askSwitch, finishSwitch })', {
    S,
    otherVersions: () => [{ id: 'hookshot' }],
    verGroup: () => ({ saves }),
    byId: (id) => ({ id, name: S.library[id].name }),
    ownName: (v) => S.library[v.id].name,
    saveUi: async (patch) => { S.settings.ui = Object.assign({}, S.settings.ui, patch); },
    ask: async () => { calls.push('ask'); return picked; },
    invoke: async (command, args) => {
      calls.push(command);
      if (command === 'saves_carry_from') return { offers, blocked };
      return command === 'saves_carry' ? S.library[args.to] : null;
    },
    uninstallNow: async (id) => { calls.push(`uninstall ${id}`); },
    ownFolder: () => true,
    savesChanged() {},
    fail: assert.fail,
    toast: (text) => toasts.push(text),
    t: String,
    tf: String,
    tn: String,
    esc: String,
    whenText: String,
  });
  return { calls, toasts, S, ...api };
}
test('installing a second version asks, and Switch Completely remembers the old one', async () => {
  const { S, askSwitch } = switching();
  assert.equal(await askSwitch('longshot'), true);
  assert.equal(S.settings.ui.switch_from.longshot, 'hookshot');
});

test('keeping both versions forgets an earlier switch', async () => {
  const { S, askSwitch } = switching({ picked: 'both' });
  S.settings.ui.switch_from = { longshot: 'hookshot' };
  assert.equal(await askSwitch('longshot'), true);
  assert.equal(S.settings.ui.switch_from.longshot, undefined);
});

test('closing the switch question installs nothing', async () => {
  const { askSwitch } = switching({ picked: null });
  assert.equal(await askSwitch('longshot'), false);
});

test('the first start after switching brings the saves over, then uninstalls the old version', async () => {
  const { calls, S, finishSwitch } = switching();
  S.settings.ui.switch_from = { longshot: 'hookshot' };
  assert.equal(await finishSwitch('longshot'), true);
  assert.deepEqual(calls, ['saves_carry_from', 'saves_carry', 'saves_backup', 'move_shortcuts', 'uninstall hookshot']);
  assert.equal(S.settings.ui.switch_from.longshot, undefined);
});

test('the old version stays when its saves cannot come along', async () => {
  const { calls, S, finishSwitch } = switching({ found: 'blocked' });
  S.settings.ui.switch_from = { longshot: 'hookshot' };
  assert.equal(await finishSwitch('longshot'), true);
  assert.deepEqual(calls, ['saves_carry_from']);
});

test('an old version with no saves yet is still uninstalled', async () => {
  const { calls, S, finishSwitch } = switching({ found: 'empty' });
  S.settings.ui.switch_from = { longshot: 'hookshot' };
  await finishSwitch('longshot');
  assert.deepEqual(calls, ['saves_carry_from', 'saves_backup', 'move_shortcuts', 'uninstall hookshot']);
});

test('saves that do not work in the new version are not copied, and the old version goes', async () => {
  const { calls, S, finishSwitch } = switching({ saves: 'no' });
  S.settings.ui.switch_from = { longshot: 'hookshot' };
  await finishSwitch('longshot');
  assert.deepEqual(calls, ['saves_backup', 'move_shortcuts', 'uninstall hookshot']);
});

test('a version keeps its own name with Show Game Names on', () => {
  const app = readFileSync(join(__dirname, '../ui/app.js'), 'utf8');
  const at = app.indexOf('const ownName =');
  const ownName = runInNewContext(app.slice(at, app.indexOf('\n', at)) + '\nownName', { S: { settings: { ui: {} } } });
  assert.equal(ownName({ id: 'longshot', name: 'The Legend of Zelda: Ocarina of Time', _name: 'Longshot' }), 'Longshot');
});

// Hookshot set up with the player's copy; Longshot about to be installed.
function bringing(precheck = async () => 'game.nup') {
  const S = { library: { hookshot: { id: 'hookshot', name: 'Hookshot', last_played: 100, game_copy: '/roms/game.nup', update_copy: '/roms/update.nsp' } } };
  const app = readFileSync(join(__dirname, '../ui/app.js'), 'utf8');
  const src = app.slice(app.indexOf('async function copyFromOther('), app.indexOf('function beginWith('));
  const copyFromOther = runInNewContext(src + '\ncopyFromOther', {
    S,
    otherVersions: () => [{ id: 'hookshot' }],
    switchFrom: () => null,
    invoke: async (command, args) => { assert.equal(command, 'precheck_copy'); return precheck(args); },
    toast() {},
    tf: String,
  });
  return { S, copyFromOther };
}

test('the copy another version was set up with comes along to a new version', async () => {
  const { S, copyFromOther } = bringing();
  await copyFromOther('longshot');
  assert.equal(S.prePicked.longshot, '/roms/game.nup');
});

test('the update another version was set up with comes along too', async () => {
  const { S, copyFromOther } = bringing();
  await copyFromOther('longshot');
  assert.equal(S.preUpdate.longshot, '/roms/update.nsp');
});

test('a copy the new version does not take stays behind', async () => {
  const { S, copyFromOther } = bringing(async () => { throw new Error('That copy reads as Europe 1.0, and this recomp is made for another version of the game.'); });
  await copyFromOther('longshot');
  assert.equal((S.prePicked || {}).longshot, undefined);
});

test('a version that has its own copy is not offered another one', async () => {
  const { S, copyFromOther } = bringing();
  S.library.longshot = { id: 'longshot', game_copy: '/roms/other.nup' };
  await copyFromOther('longshot');
  assert.equal((S.prePicked || {}).longshot, undefined);
});

test('switching says so before the saves are copied', async () => {
  const { toasts, S, finishSwitch } = switching();
  S.settings.ui.switch_from = { longshot: 'hookshot' };
  await finishSwitch('longshot');
  assert.equal(toasts[0], 'Switching from {old} to {name}…');
});

test('a game page redraw updates its versions pill, so an uninstalled version stops counting', () => {
  const app = readFileSync(join(__dirname, '../ui/app.js'), 'utf8');
  const at = app.indexOf('function refreshGame()');
  const refresh = app.slice(at, app.indexOf('\n}\n', at));
  assert.match(refresh, /pill\.outerHTML = verPill\(a\)/);
});
