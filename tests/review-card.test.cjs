// The Reviews card on a game's page: the player's own review on top, rows that filter by answer,
// more reviews on the page, and the review dialog starting from what was said before.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8').replace(/\r\n/g, '\n');
const app = read('app.js'), more = read('more.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const line = (src, name) => src.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];

test('the player\'s own review is found by the game it\'s about', () => {
  const c = vm.createContext({ S: { myReports: { list: [{ app: 'karpur', verdict: 'runs' }] } }, window: {} });
  vm.runInContext(`${line(app, 'myReviewOf')}\nglobalThis.mine = myReviewOf;`, c);
  assert.equal(c.mine('karpur').verdict, 'runs');
  assert.equal(c.mine('longshot'), null);
});

test('the score is colored by how good it is', () => {
  const c = vm.createContext({});
  vm.runInContext(`${line(app, 'scoreTone')}\nglobalThis.tone = scoreTone;`, c);
  assert.equal(c.tone(80), 'good');
  assert.equal(c.tone(50), 'mid');
  assert.equal(c.tone(20), 'low');
});

test('reviewed already, the card\'s button is Edit Your Review', () => {
  assert.match(fn(app, 'runsCard'), /mine \? t\('Edit Your Review'\) : t\('Write a Review'\)/);
  assert.match(fn(app, 'runsCard'), /\$\{myReviewBlock\(a\.id\)\}/);
});

test('the card keeps the score to one line, and the rows for each answer are in All Reviews', () => {
  const card = fn(app, 'runsCard');
  assert.match(card, /\$\{runsLine\(rep\)\}/);
  assert.doesNotMatch(card, /runRows|runsSummary/);
  assert.match(fn(app, 'runsLine'), /\$\{runBar\(sc\.r\)\}<\/div>`;/);
  assert.match(fn(app, 'reviewsModal'), /\$\{runsSummary\(rep, pickV\)\}/);
});

test('in All Reviews a row narrows the list, and one nobody gave can\'t be picked', () => {
  const rows = fn(app, 'runRows');
  assert.match(rows, /data-x="v" data-v="\$\{k\}"/);
  assert.match(rows, /\$\{r\[k\] \|\| on === k \? '' : ' disabled'\}/);
  assert.match(fn(app, 'reviewsModal'), /\(!pickV \|\| x\.verdict === pickV\)/);
});

test('the player\'s own review is on the card only, not in All Reviews', () => {
  assert.doesNotMatch(fn(app, 'reviewsModal'), /myReviewBlock/);
});

test('the card shows the three newest, and All Reviews has the rest in the launcher', () => {
  const card = fn(app, 'runsCard');
  assert.match(card, /const newest = items\.slice\(0, 3\)\.map\(repItem\)\.join\(''\);/);
  assert.match(card, /items\.length > 3\n\s*\? `<button type="button" class="btn" data-act="rep-all"/);
  assert.doesNotMatch(card, /rep-more|Show More Reviews/);
  assert.match(app, /case 'rep-all': return reviewsModal\(id\);/);
});

test('All Reviews says when the site has more than the newest it sent', () => {
  assert.match(fn(app, 'reviewsModal'), /all\.n > items\.length \? `<p class="rva-rest">/);
});

test('a review says where it was played', () => {
  const c = vm.createContext({ esc: (s) => String(s), DECK_ICON: () => '<svg></svg>', osIcon: (k) => `<i data-os="${k}"></i>` });
  vm.runInContext(`${line(app, 'REP_SYS')}\n${fn(app, 'repSys')}\nglobalThis.sys = repSys;`, c);
  assert.match(c.sys({ sys: 'deck' }), /Steam Deck/);
  assert.match(c.sys({ sys: 'linux' }), /data-os="linux".*Linux/);
  assert.equal(c.sys({ sys: 'amiga' }), '');
});

test('the review dialog names the project, not the game Show Game Names puts on its page', () => {
  const apps = { karpur: { name: 'Pokemon Scarlet and Violet', _name: 'KarPur' }, longshot: { name: 'Longshot' } };
  const c = vm.createContext({ byId: (id) => apps[id] });
  vm.runInContext(`${fn(more, 'reviewName')}\nglobalThis.name = reviewName;`, c);
  assert.equal(c.name('karpur', { name: 'KarPur (installed)' }), 'KarPur');
  assert.equal(c.name('longshot', { name: 'x' }), 'Longshot');
  assert.equal(c.name('gone', { name: 'From the Library' }), 'From the Library');
  assert.match(fn(more, 'reviewDialog'), /const own = \(k\) => pickArt\(a, k, false\);/);
});

test('changing a review starts the dialog from the answer and note given before', () => {
  assert.match(fn(more, 'askRun'), /const prev = asked_for \? myReviewOf\(id\) : null;/);
  const dialog = fn(more, 'reviewDialog');
  // One page: the answer given before is picked already, and the words are in the field.
  assert.match(dialog, /let verdict = prev \? prev\.verdict : null/);
  assert.match(dialog, /let draft = prev && prev\.note \? prev\.note : ''/);
  assert.match(dialog, />\$\{esc\(draft\)\}<\/textarea>/);
  assert.match(dialog, /const sendLabel = editing \? t\('Update Review'\) : t\('Send'\);/);
});

test('the review window is one page, and its X closes it like Cancel or Not Now', () => {
  const dialog = fn(more, 'reviewDialog');
  assert.match(dialog, /const s = modal\(`\$\{art\}\$\{one\(\)\}`/);
  assert.doesNotMatch(dialog, /const second = |const first = /);
  assert.match(dialog, /class="rv-x" data-x="cancel"/);
  assert.match(dialog, /data-x="cancel">\$\{esc\(auto \? t\('Not Now'\) : t\('Cancel'\)\)\}/);
  assert.match(dialog, /if \(act === 'cancel'\) \{ if \(busy\) return; delete s\.dataset\.busy; return close\(\); \}/);
});

test('Delete Review shows only when changing a review already sent, never right after a session', () => {
  const dialog = fn(more, 'reviewDialog');
  assert.match(dialog, /const left = auto \? `<label class="rv-never">/);
  assert.match(dialog, /: prev \? `<button type="button" class="btn danger rv-del" data-x="del">\$\{esc\(t\('Delete Review'\)\)\}<\/button>` : '<span><\/span>';/);
  assert.match(dialog, /if \(act === 'del'\) \{[^\n]*body\(\)\.outerHTML = confirmDel\(\); return; \}/);
});

test('deleting a review asks the site, then takes it out of Your Reviews and keeps it hidden', async () => {
  const calls = [], gone = [];
  const mine = { app: 'karpur', verdict: 'runs' };
  const S = { myReports: { list: [mine, { app: 'longshot' }] }, reports: { karpur: {} }, view: 'home' };
  const c = vm.createContext({ S, invoke: async (cmd, args) => { calls.push([cmd, args]); return true; }, toast: () => {}, t: (s) => s, loadReports: () => {}, render: () => {}, refreshGame: () => {}, window: {}, setTimeout,
    myReviewOf: (id) => S.myReports.list.find((x) => x.app === id) || null, goneReview: (id, r) => gone.push([id, r]) });
  vm.runInContext(`${fn(more, 'deleteReview')}\nglobalThis.del = deleteReview;`, c);
  await c.del('karpur');
  assert.equal(JSON.stringify(calls), JSON.stringify([['review_delete', { id: 'karpur' }]]));
  assert.equal(S.myReports.list.map((x) => x.app).join(), 'longshot');
  assert.deepEqual(gone, [['karpur', mine]]);
});

// The website's lists can be cached for minutes after a delete (Cloudflare): the review must not come back.
const goneKit = (S) => {
  const c = vm.createContext({ S, Date, Object, JSON, Math, String, Array });
  vm.runInContext(`${line(app, 'verNorm')}\n${line(app, 'GONE_MS')}\n${fn(app, 'reviewGone')}\n${app.match(/^const sameReview = [^]*?;\n/m)[0]}${line(app, 'lessOne')}\n${fn(app, 'goneReview')}\n${fn(app, 'keepGoneOut')}\n${fn(app, 'keepGoneCounts')}\nObject.assign(globalThis, { goneReview, keepGoneOut, keepGoneCounts, reviewGone });`, c);
  return c;
};
const MINE = { app: 'karpur', verdict: 'runs', note: 'Smooth', version: 'v0.4.0', specs: 'PC' };
const OTHER = { verdict: 'issues', note: 'Crackles', version: 'v0.4.0', specs: 'PC2' };

test('a deleted review stays out of a cached list, and its answer counts one less', () => {
  const S = { reports: { karpur: { counts: { runs: 5, issues: 1 }, items: [Object.assign({}, MINE), OTHER] } }, runCounts: { karpur: { runs: 5, issues: 1 } }, latestCounts: {} };
  const c = goneKit(S);
  c.goneReview('karpur', MINE);
  assert.equal(S.reports.karpur.items.length, 1);
  assert.equal(S.reports.karpur.counts.runs, 4);
  assert.equal(S.runCounts.karpur.runs, 4);
  // The cache sends the old list again: still hidden, still one less.
  const again = c.keepGoneOut('karpur', { counts: { runs: 5, issues: 1 }, items: [Object.assign({}, MINE), OTHER] });
  assert.equal(again.items.length, 1);
  assert.equal(again.counts.runs, 4);
  S.runCounts = { karpur: { runs: 5, issues: 1 } };
  c.keepGoneCounts();
  assert.equal(S.runCounts.karpur.runs, 4);
});

test('a fresh list without the deleted review ends the hiding', () => {
  const S = { reports: { karpur: { counts: { runs: 5 }, items: [Object.assign({}, MINE)] } }, runCounts: {}, latestCounts: {} };
  const c = goneKit(S);
  c.goneReview('karpur', MINE);
  const fresh = c.keepGoneOut('karpur', { counts: { runs: 4 }, items: [OTHER] });
  assert.equal(fresh.counts.runs, 4);
  assert.equal(c.reviewGone('karpur'), null);
});
