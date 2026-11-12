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
    window: {},
  });
  c.group = (title, body) => `<h3>${title}</h3>${body}`;
  c.row = (b, s, ctl) => `<div><b>${b}</b><span>${s}</span>${ctl}</div>`;
  c.sw = (k, value, label) => `<button aria-checked="${!!value}" aria-label="${label}" data-act="toggle" data-k="${k}"></button>`;
  vm.runInContext(fn(app, 'steamSyncLine') + fn(app, 'steamSyncGroup') + fn(app, 'steamSync'), c);
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

test('Big Picture shows the setting, and Sync Now once it is on', () => {
  const c = vm.createContext({
    S: { steam: true, settings: { steam_sync: false, ui: {} }, noSound: true },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '',
  });
  vm.runInContext(fn(tv, 'pageSettings', '  '), c);
  assert.match(c.pageSettings(), /data-toggle="steam_sync" data-sw="0"><span class="k">Keep Steam in Sync/);
  assert.doesNotMatch(c.pageSettings(), /data-tvact="steamsync"/);
  c.S.settings.steam_sync = true;
  assert.match(c.pageSettings(), /data-tvact="steamsync"/);
});
