const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name, indent = '') => src.match(new RegExp(`^${indent}(?:async )?function ${name}\\([^]*?^${indent}}`, 'm'))[0];

function context(on, state) {
  const c = vm.createContext({
    S: { steam: true, settings: { steam_sync: on, ui: {} }, steamSyncState: state, view: 'settings', setTab: 'library' },
    t: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    tn: (n, one, many) => (n === 1 ? one : many).replace('{n}', n),
    render: () => {}, setSection: () => {}, toast: () => {},
    fail: (error) => { throw error; },
    invoke: async () => state,
    saveUi: async () => {},
    document: { body: { classList: { contains: () => false } } },
    window: {},
  });
  c.group = (title, body) => `<h3>${title}</h3>${body}`;
  c.row = (b, s, ctl) => `<div><b>${b}</b><span>${s}</span>${ctl}</div>`;
  c.sw = (k, value, label) => `<button aria-checked="${!!value}" aria-label="${label}" data-act="toggle" data-k="${k}"></button>`;
  const fns = ['steamSyncLine', 'steamSyncGroup', 'steamSync', 'steamRemoveRow', 'askSteamRemove', 'steamRemoveAll'];
  vm.runInContext(fns.map((name) => fn(app, name)).join('\n'), c);
  return c;
}

test('the setting is a switch, off by default, with no Sync Now', () => {
  const c = context(false);
  const html = c.steamSyncGroup(c.group, c.row, c.sw);
  assert.match(html, /aria-checked="false" aria-label="Keep Steam in Sync" data-act="toggle" data-k="steam_sync"/);
  assert.doesNotMatch(html, /data-act="steam-sync"/);
});

test('turned on, Sync Now says how many changes wait for Steam to close', () => {
  const c = context(true, { steam: true, games: 3, add: 1, remove: 1, collection: false });
  const html = c.steamSyncGroup(c.group, c.row, c.sw);
  assert.match(html, /data-act="steam-sync"/);
  assert.match(html, /2 changes wait for Steam to close/);
});

test('turned on and in sync, it says Steam has every installed game', () => {
  const c = context(true, { steam: true, games: 3, add: 0, remove: 0, collection: false });
  assert.match(c.steamSyncLine(), /Steam has all 3 of your installed games/);
});

test('with Steam open, Sync Now asks before restarting it', async () => {
  const c = context(true);
  const calls = [];
  c.invoke = async (command, args) => { calls.push(args.restart); return args.restart ? 'synced:1:0' : 'steam_running'; };
  c.window.ask = async () => 'yes';
  await c.steamSync(false);
  assert.deepEqual(calls, [false, true]);
});

test('with Steam open and Later picked, nothing is restarted', async () => {
  const c = context(true);
  const calls = [];
  c.invoke = async (command, args) => { calls.push(args.restart); return 'steam_running'; };
  c.window.ask = async () => null;
  await c.steamSync(false);
  assert.deepEqual(calls, [false]);
});

test('Remove from Steam shows with sync off, counting the launcher\'s games', () => {
  const c = context(false, { steam: true, ours: 4, launcher: false });
  const html = c.steamSyncGroup(c.group, c.row, c.sw);
  assert.match(html, /<b>Remove from Steam<\/b><span>Takes out the 4 games RecompLauncher put in Steam/);
  assert.doesNotMatch(html, /Keep Steam in Sync turns off too/);
  assert.doesNotMatch(html, /data-act="steam-remove" disabled/);
});

test('with sync on, Remove from Steam says sync turns off too', () => {
  const c = context(true, { steam: true, games: 4, add: 0, remove: 0, ours: 4 });
  assert.match(c.steamRemoveRow(c.row), /Keep Steam in Sync turns off too, or they'd come back\./);
});

test('with nothing from the launcher in Steam, Remove All is disabled', () => {
  const c = context(false, { steam: true, ours: 0, launcher: false });
  const html = c.steamRemoveRow(c.row);
  assert.match(html, /Steam has no games from RecompLauncher\./);
  assert.match(html, /data-act="steam-remove" disabled/);
});

test('Remove All asks first, and cancelling removes nothing', async () => {
  const c = context(true, { steam: true, ours: 2, launcher: false });
  const calls = [];
  c.invoke = async (command, args) => { calls.push(command); return 'removed:2:0'; };
  c.window.ask = async () => null;
  await c.steamRemoveAll();
  assert.deepEqual(calls, []);
});

test('Remove All with Steam open restarts it only after asking', async () => {
  const c = context(true, { steam: true, ours: 2, launcher: false });
  const restarts = [];
  c.invoke = async (command, args) => { restarts.push(args.restart); return args.restart ? 'removed:2:0' : 'steam_running'; };
  const answers = ['games', 'yes'];
  c.window.ask = async () => answers.shift();
  await c.steamRemoveAll();
  assert.deepEqual(restarts, [false, true]);
  assert.equal(c.S.settings.steam_sync, false);
});

test('in Big Picture, the launcher\'s own entry goes only with the third button', async () => {
  const c = context(false, { steam: true, ours: 2, launcher: true });
  c.document.body.classList.contains = () => true;
  const sent = [];
  c.invoke = async (command, args) => { sent.push(args.launcher); return 'removed:2:1'; };
  let buttons;
  c.window.ask = async (title, body, list) => { buttons = list; return 'both'; };
  await c.steamRemoveAll();
  assert.equal(buttons[2].label, 'Remove Them and RecompLauncher');
  assert.deepEqual(sent, [true]);
});

test('Big Picture shows the setting, and Sync Now once it is on', () => {
  const c = vm.createContext({
    S: { steam: true, settings: { steam_sync: false, ui: {} }, noSound: true },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, bgMode: () => '', langName: () => 'English', clockSelect: () => '', clockRow: () => '', setClock: () => {},
    svg: () => '', hints: () => '', HEART: '',
  });
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="steam_sync" data-sw="0"><span class="k">Keep Steam in Sync/);
  assert.doesNotMatch(c.pageSettings(), /data-tvact="steamsync"/);
  assert.match(c.pageSettings(), /data-tvact="steamremove"><span class="k">Remove from Steam/);
  c.S.settings.steam_sync = true;
  assert.match(c.pageSettings(), /data-tvact="steamsync"/);
});
