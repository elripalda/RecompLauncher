const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const topFn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];
const topConst = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];
const tvFn = (name) => tv.match(new RegExp(`^  (?:async )?function ${name}\\([^]*?^  }`, 'm'))[0];

const helpers = {
  curLocale: () => 'en-US',
  timeOpts: (o) => Object.assign({}, o, { hourCycle: 'h23' }),
  t: (s) => s,
  esc: String,
  tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
  tn: (n, one, many) => (n === 1 ? one : many.replace('{n}', n)),
};

function appContext() {
  const c = vm.createContext({ ...helpers });
  vm.runInContext(topFn('latestPerGame') + topFn('dayOf') + topConst('clockOf') + topFn('didWhat') + 'globalThis.clockOf = clockOf;', c);
  return c;
}

const at = (daysAgo, h, m) => {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo, h, m).getTime() / 1000;
};

test('Recent keeps only the newest download of a game installed three times', () => {
  const hist = [
    { id: 'lostodyssey', when: 30 },
    { id: 'lostodyssey', when: 20 },
    { id: 'odyssey', when: 15 },
    { id: 'lostodyssey', when: 10 },
  ];
  const rows = appContext().latestPerGame(hist);
  assert.deepEqual(rows.map((h) => `${h.id}@${h.when}`), ['lostodyssey@30', 'odyssey@15']);
});

test('a download finished today goes under today', () => {
  assert.equal(appContext().dayOf(at(0, 10, 1)), 'today');
});

test('a download finished late last night goes under yesterday', () => {
  assert.equal(appContext().dayOf(at(1, 23, 50)), 'yesterday');
});

test('an older download goes under its date', () => {
  assert.doesNotMatch(appContext().dayOf(at(5, 9, 5)), /today|yesterday/);
});

test('a row shows only the time, in the player\'s clock', () => {
  assert.equal(appContext().clockOf(at(0, 9, 5)), '09:05');
});

test('an update says which version it came from and went to', () => {
  assert.equal(appContext().didWhat({ update: true, from: 'v1.0.0', tag: 'v1.1.0' }), 'Updated v1.0.0 → v1.1.0');
});

test('a fresh install says the version it installed', () => {
  assert.equal(appContext().didWhat({ update: false, tag: 'v1.1.0' }), 'Installed v1.1.0');
});

test('Big Picture lists a game installed three times once', () => {
  const c = vm.createContext({
    ...helpers,
    S: { jobs: {}, recent: [], updates: {}, library: {}, dlHist: [], launcherUpd: null, version: '0.90.0-beta.67' },
    verName: (v) => v, thumbOf: String,
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s, hints: () => '',
    libAppOf: (id) => ({ id, name: id }), media: String, SCAN_TIP: '', progress: () => '', cleanMsg: String,
    scanInfo: () => ({ cls: 'ok', ic: '✓', text: 'Safe' }),
  });
  vm.runInContext(topFn('latestPerGame') + tvFn('scanTag') + tvFn('pageDownloads'), c);
  c.S.dlHist = [1, 2, 3].map((n) => ({ id: 'lostodyssey', tag: 'v1.1.0', when: n }));
  const html = c.pageDownloads();
  assert.equal(html.match(/data-key="scan:lostodyssey@v1\.1\.0"/g).length, 1);
});

const CATALOG = {
  lostodyssey: { name: 'Lost Odyssey', repo: 'freefrank/LostOdysseyRecomp' },
  odyssey: { name: 'Odyssey Recomp', repo: 'aboood40091/odyssey' },
  mine: { name: 'Mine', repo: 'me/mine' },
};

// The desktop Downloads page, drawn for real with plain stand-ins for its helpers.
function desktopPage(hist, library, more = {}) {
  const c = vm.createContext({
    ...helpers,
    S: { jobs: {}, paused: {}, recent: [], updates: {}, library, dlHist: hist, settings: { ui: {} }, launcherUpd: null, ...more },
    fmtB: String, byId: (id) => CATALOG[id], scanPill: () => '', pic: () => '', thumbOf: String,
  });
  const code = ['INFO_I', 'CHEV_D', 'when', 'clockOf'].map(topConst).join('\n') + topFn('latestPerGame') + topFn('dayOf') + topFn('didWhat') + topFn('pageDownloads');
  vm.runInContext(code, c);
  return c.pageDownloads();
}

test('a Recent row has the time and a quiet Patch Notes button that opens the game\'s Updates tab', () => {
  const html = desktopPage([{ id: 'lostodyssey', tag: 'v1.1.0', when: at(0, 10, 1) }], { lostodyssey: { tag: 'v1.1.0' } });
  assert.match(html, /<span class="dl-time">10:01<\/span><button type="button" class="dl-notes" data-act="upd-game" data-id="lostodyssey"/);
});

test('a local build has no releases, so its Recent row has no Patch Notes', () => {
  const html = desktopPage([{ id: 'mine', tag: 'own', when: at(0, 9, 0) }], { mine: { tag: 'own', local: true } });
  assert.doesNotMatch(html, /data-act="upd-game"/);
  assert.match(html, /<span class="dl-notes" aria-hidden="true"><\/span>/);
});

test('Recent puts each day under its own header', () => {
  const html = desktopPage([{ id: 'lostodyssey', tag: 'v1.1.0', when: at(0, 10, 1) }, { id: 'odyssey', tag: '0.2.6', when: at(5, 9, 0) }], {});
  assert.equal(html.match(/<div class="dl-day">/g).length, 2);
  assert.match(html, /<div class="dl-day">today<\/div>/);
});

test("a game's older downloads fold under its row, closed at first", () => {
  const hist = [3, 2, 1].map((n) => ({ id: 'lostodyssey', tag: 'v1.1.0', update: n > 1, from: 'v1.0.0', when: at(0, 10, n) }));
  const html = desktopPage(hist, {});
  assert.match(html, /data-act="dl-earlier" data-id="lostodyssey" aria-expanded="false">2 earlier downloads/);
  assert.doesNotMatch(html, /class="dl-earlier"/);
});

test("an opened game shows each of its older downloads", () => {
  const hist = [3, 2, 1].map((n) => ({ id: 'lostodyssey', tag: 'v1.1.0', update: n > 1, from: 'v1.0.0', when: at(0, 10, n) }));
  const html = desktopPage(hist, {}, { dlOpen: new Set(['lostodyssey']) });
  assert.match(html, /<div class="dl-earlier"><div><span>Updated v1\.0\.0 → v1\.1\.0<\/span><small>[^<]+<\/small><\/div><div><span>Installed v1\.1\.0<\/span>/);
});

test('Downloads is one page: no tabs and no launcher banner, even with a launcher update', () => {
  const html = desktopPage([{ id: 'lostodyssey', tag: 'v1.1.0', when: at(0, 10, 1) }], {}, { launcherUpd: { version: '0.90.0-beta.68' } });
  assert.doesNotMatch(html, /data-act="dl-tab"/);
  assert.doesNotMatch(html, /self-update|RecompLauncher/);
});

test('the totals are one line above the lists', () => {
  const hist = [{ id: 'lostodyssey', tag: 'v1.1.0', update: true, size: 100, secs: 2, when: at(0, 10, 1) }];
  const html = desktopPage(hist, {});
  assert.match(html, /<p class="dl-totals">100 downloaded · 1 download · 50\/s average · 1 update installed<\/p>/);
});
