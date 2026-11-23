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

test('a row for each answer: on the card it opens All Reviews, in that window it narrows the list', () => {
  const rows = fn(app, 'runRows');
  assert.match(rows, /inWindow \? `data-x="v" data-v="\$\{k\}"` : `data-act="rep-v" data-id="\$\{esc\(id\)\}" data-v="\$\{k\}"`/);
  assert.match(rows, /\$\{r\[k\] \|\| on === k \? '' : ' disabled'\}/);
  assert.match(app, /case 'rep-v': return reviewsModal\(id, el\.dataset\.v\);/);
  assert.match(fn(app, 'reviewsModal'), /\(!pickV \|\| x\.verdict === pickV\)/);
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
  assert.match(dialog, /t\('Your Last Answer'\)/);
  assert.match(dialog, />\$\{prev && prev\.note \? esc\(prev\.note\) : ''\}<\/textarea>/);
});
