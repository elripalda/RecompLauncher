const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const line = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];
const esc = (s) => String(s == null ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function ctx(extra = {}) {
  const c = vm.createContext({
    S: { settings: { ui: {} }, library: {}, running: new Set(), trend: undefined },
    esc, t: (s) => s, tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]), fmtNum: String,
    document: { body: { classList: { contains: () => false } }, querySelector: () => null },
    ...extra,
  });
  return c;
}

test('pictures are asked for at the widths the site makes', () => {
  const c = ctx();
  vm.runInContext(`${line('THUMB_W')}\n${line('thumbW')}\nglobalThis.thumbW = thumbW;`, c);
  assert.equal(c.thumbW(160), 160);
  assert.equal(c.thumbW(300), 320);
  assert.equal(c.thumbW(500), 640);
  assert.equal(c.thumbW(2000), 960);
  assert.equal(c.thumbW(undefined), 400);
});

test("the Trending row keeps the chart's ranks and skips games this player can't see", () => {
  const apps = { a: { id: 'a' }, b: { id: 'b' }, soon: { id: 'soon', soon: true } };
  const c = ctx({ byId: (id) => apps[id] });
  vm.runInContext(fn('trendList') + fn('trendChip'), c);
  c.S.trend = { first: false, entries: [{ id: 'gone', rank: 1 }, { id: 'b', rank: 2, last: 5, move: 'up' }, { id: 'soon', rank: 3 }, { id: 'a', rank: 4, last: 2, move: 'down' }] };
  const list = c.trendList([apps.a, apps.b, apps.soon]);
  assert.deepEqual(list.map((x) => [x.a.id, x.e.rank]), [['b', 2], ['a', 4]]);
  assert.match(c.trendChip(list[0].e), /class="tchip up"[^>]*>▲ 3</);
  assert.match(c.trendChip(list[1].e), /class="tchip down"[^>]*>▼ 2</);
  assert.match(c.trendChip({ move: 'new', rank: 1 }), /tchip new">New Entry</);
  assert.match(c.trendChip({ move: 're', rank: 1 }), /tchip re">Re-entry</);
  assert.match(c.trendChip({ move: 'same', rank: 1, last: 1 }), /tchip same/);
  // The very first chart has no movement to show.
  c.S.trend.first = true;
  assert.equal(c.trendChip(list[0].e), '');
});

test('the partner card shows only when the site turns it on for the launcher', () => {
  const c = ctx();
  vm.runInContext(app.match(/^const IG = .*$/m)[0] + '\n' + line('igOn') + fn('igEntry'), c);
  const a = { id: 'oot' };
  vm.runInContext("IG.data = { on: true, launcher: false, l: { oot: { t: 'The Game', price: '$10' } } }", c);
  assert.equal(c.igEntry(a), null);
  vm.runInContext('IG.data.launcher = true', c);
  assert.equal(c.igEntry(a).t, 'The Game');
  assert.equal(c.igEntry({ id: 'other' }), null);
  vm.runInContext('IG.data.l.oot.off = true', c);
  assert.equal(c.igEntry(a), null);
  vm.runInContext('IG.data.l.oot.off = false', c);
  c.S.settings.ui.hide_buy = true;
  assert.equal(c.igEntry(a), null);
});

test('how are you enjoying it waits for real play, and is asked only once', () => {
  const c = ctx();
  vm.runInContext(app.match(/^const ENJOY = .*$/m)[0] + fn('enjoyDue'), c);
  c.S.settings.setup_done = true;
  c.S.library = { a: { played_secs: 3000, sessions: [1, 2] }, b: { played_secs: 5000, sessions: [1] } };
  c.S.settings.ui.enjoy_days = ['2026-11-13'];
  assert.equal(c.enjoyDue(), false);
  c.S.settings.ui.enjoy_days = ['2026-11-13', '2026-11-14'];
  assert.equal(c.enjoyDue(), true);
  c.S.running.add('a');
  assert.equal(c.enjoyDue(), false);
  c.S.running.clear();
  c.S.settings.ui.enjoy_done = 'up';
  assert.equal(c.enjoyDue(), false);
});

test('a poster with a versions count keeps one label at the bottom', () => {
  const src = fn('poster');
  assert.match(src, /needsSetup\(a\) && !a\.soon && !setupDone\(a\.id\) \? \(vg \? '' :/);
  assert.match(src, /turnkey\(a\) \? \(vg \? `<span class="badge easy ico"/);
});

test('the Trending row stands in for Popular right now, which comes back without a chart', () => {
  const src = fn('trendShelf');
  assert.match(src, /S\.trend === undefined\) return `<section class="shelf trend-wait"/);
  assert.match(src, /if \(!list\.length\) return fallback\(\);/);
});
