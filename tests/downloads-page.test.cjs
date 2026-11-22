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
    S: { jobs: {}, paused: {}, recent: [], updates: {}, library, dlHist: hist, settings: { ui: {} }, launcherUpd: null, building: null, buildQ: [], setupJobs: {}, ...more },
    fmtB: String, byId: (id) => CATALOG[id], scanPill: () => '', pic: () => '', thumbOf: String,
    pct: () => 0, dlLine: (id) => `line of ${id}`, dlTarget: () => 'Update to v2', buildLine: () => 'Building the game · 3%', buildPct: () => 3,
  });
  const consts = ['INFO_I', 'CHEV_D', 'XS', 'when', 'clockOf', 'canPause'].map(topConst).join('\n');
  const code = consts + topFn('latestPerGame') + topFn('dayOf') + topFn('didWhat') + topFn('progLabel') + topFn('progress') + topFn('pageDownloads');
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

test("a game downloaded several times is one Recent row, with nothing folded under it", () => {
  const hist = [3, 2, 1].map((n) => ({ id: 'lostodyssey', tag: 'v1.1.0', update: n > 1, from: 'v1.0.0', when: at(0, 10, n) }));
  const html = desktopPage(hist, {});
  assert.equal(html.match(/<b>Lost Odyssey<\/b>/g).length, 1);
  assert.doesNotMatch(html, /earlier/);
});

test('Downloads is one page: no tabs and no launcher banner, even with a launcher update', () => {
  const html = desktopPage([{ id: 'lostodyssey', tag: 'v1.1.0', when: at(0, 10, 1) }], {}, { launcherUpd: { version: '0.90.0-beta.68' } });
  assert.doesNotMatch(html, /data-act="dl-tab"/);
  assert.doesNotMatch(html, /self-update|RecompLauncher/);
});

test('the totals line is gone: each download shows its own numbers', () => {
  const hist = [{ id: 'lostodyssey', tag: 'v1.1.0', update: true, size: 100, secs: 2, when: at(0, 10, 1) }];
  assert.doesNotMatch(desktopPage(hist, {}), /dl-totals|average/);
});

// Lost Odyssey downloading, Odyssey waiting its turn, Mine held back for the build before it.
const MOVING = { lostodyssey: { id: 'lostodyssey', phase: 'download', done: 50, total: 100 }, odyssey: { id: 'odyssey', phase: 'queued' }, mine: { id: 'mine', phase: 'queued', held: true } };

test('every download is one Downloading list, with no Up Next', () => {
  const html = desktopPage([], {}, { jobs: MOVING });
  assert.match(html, /<h2>Downloading<span class="dl-n">3<\/span><\/h2>/);
  assert.doesNotMatch(html, /Up Next|dl-hero/);
});

test('the download that is moving comes first, with its line and progress', () => {
  const html = desktopPage([], {}, { jobs: MOVING });
  assert.match(html, /<div class="dl-list"><div class="dl-row dl-moving">.*?<b>Lost Odyssey<\/b><span class="dl-phase" data-dl-phase="lostodyssey">line of lostodyssey<\/span><div class="prog" data-prog="lostodyssey">/);
});

test('a download waiting its turn is a quiet row that says what it will be, Queued, and a cancel cross', () => {
  const html = desktopPage([], {}, { jobs: MOVING });
  assert.match(html, /<b>Odyssey Recomp<\/b><span class="dl-sub"><span>Update to v2<\/span><\/span><\/div><span class="dl-state">Queued<\/span><button type="button" class="btn sm sq" data-act="cancel" data-id="odyssey" aria-label="Cancel"/);
  assert.doesNotMatch(html, /data-prog="odyssey"/);
});

test('an update held back for a build says it waits for the build before it', () => {
  const html = desktopPage([], {}, { jobs: MOVING });
  assert.match(html, /<b>Mine<\/b><span class="dl-sub"><span>Update to v2 · Waiting for the build before it<\/span>/);
});

test('a build right after an update is in the list, moving, with a cross that stops the build', () => {
  const html = desktopPage([], {}, { building: 'odyssey' });
  assert.match(html, /<h2>Downloading<span class="dl-n">1<\/span><\/h2>/);
  assert.match(html, /<div class="dl-row dl-moving">.*?<span class="dl-phase" data-dl-phase="odyssey">Building the game · 3%<\/span><div class="prog" data-build-prog="odyssey"><div class="track"><i style="width:3%">/);
  assert.match(html, /data-act="build-cancel" data-id="odyssey"/);
});

test('the build comes after the downloads that are moving and before the ones waiting', () => {
  const html = desktopPage([], {}, { jobs: MOVING, building: 'odyssey2' });
  const order = [...html.matchAll(/data-act="(?:cancel|build-cancel)" data-id="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(order, ['lostodyssey', 'odyssey2', 'odyssey', 'mine']);
});

test('a game queued to build says it waits to build', () => {
  const html = desktopPage([], {}, { buildQ: ['lostodyssey'] });
  assert.match(html, /<b>Lost Odyssey<\/b><span class="dl-sub"><span>Waiting to build<\/span>/);
});

test('Pause all shows while something can be paused', () => {
  assert.match(desktopPage([], {}, { jobs: MOVING }), /data-act="dl-pause-all">Pause all</);
});

test('no Pause all when only updates held for a build are waiting', () => {
  const html = desktopPage([], {}, { jobs: { mine: MOVING.mine } });
  assert.doesNotMatch(html, /dl-pause-all/);
});

// The moving download's one line, with the real helpers it uses.
function lineContext(jobs, library = {}, updates = {}) {
  const c = vm.createContext({ ...helpers, S: { jobs, library, updates }, fmtB: String, pct: () => 0 });
  vm.runInContext('const DLSPD = { g: [10] };' + topConst('took') + topFn('progLabel') + topFn('dlPhase') + topFn('dlTarget') + topFn('dlLine'), c);
  return c;
}

test("the moving download's line has the version, sizes, speed and time left", () => {
  const c = lineContext({ g: { phase: 'download', done: 0, total: 100, speed: 1 } }, { g: { tag: 'v1' } }, { g: 'v2' });
  assert.equal(c.dlLine('g'), 'Update to v2 · 0 of 100 · 10/s · 10 sec left');
});

test('a first install says Install on its line', () => {
  const c = lineContext({ g: { phase: 'download', done: 0, total: 100, speed: 1 } });
  assert.match(c.dlLine('g'), /^Install · /);
});

test('before its bytes come, the line says what the download is doing', () => {
  const c = lineContext({ g: { phase: 'unpack', done: 0, total: 0 } });
  assert.equal(c.dlLine('g'), 'Install · Unpacking');
});
