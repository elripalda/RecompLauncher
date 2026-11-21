const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const topFn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];
const tvFn = (name) => tv.match(new RegExp(`^  (?:async )?function ${name}\\([^]*?^  }`, 'm'))[0];

function appContext() {
  const c = vm.createContext({
    curLocale: () => 'en-US',
    timeOpts: (o) => Object.assign({}, o, { hourCycle: 'h23' }),
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
  });
  vm.runInContext(topFn('latestPerGame') + topFn('doneAt'), c);
  return c;
}

const at = (daysAgo, h, m) => {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo, h, m).getTime() / 1000;
};

test('Completed keeps only the newest download of a game installed three times', () => {
  const hist = [
    { id: 'lostodyssey', when: 30 },
    { id: 'lostodyssey', when: 20 },
    { id: 'odyssey', when: 15 },
    { id: 'lostodyssey', when: 10 },
  ];
  const rows = appContext().latestPerGame(hist);
  assert.deepEqual(rows.map((h) => `${h.id}@${h.when}`), ['lostodyssey@30', 'odyssey@15']);
});

test('a download finished today reads Today and its time', () => {
  assert.equal(appContext().doneAt(at(0, 10, 1)), 'Today 10:01');
});

test('a download finished late last night reads Yesterday', () => {
  assert.equal(appContext().doneAt(at(1, 23, 50)), 'Yesterday 23:50');
});

test('an older download reads its date and time', () => {
  const text = appContext().doneAt(at(5, 9, 5));
  assert.match(text, /09:05/);
  assert.doesNotMatch(text, /Today|Yesterday/);
});

test('Big Picture lists a game installed three times once', () => {
  const c = vm.createContext({
    S: { jobs: {}, recent: [], updates: {}, library: {}, dlHist: [], launcherUpd: null, version: '0.90.0-beta.67' },
    t: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
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
