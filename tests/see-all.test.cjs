const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js'), css = read('app.css');

// A function declaration out of app.js, by name.
function fn(src, name) {
  const start = src.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  let i = src.indexOf('{', start), depth = 0;
  for (; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) break;
  }
  return src.slice(start, i + 1);
}

// One top-level `const name = …;` line out of app.js.
function line(name) {
  const m = app.match(new RegExp(`^const ${name} = .*$`, 'm'));
  assert.ok(m, name);
  return m[0];
}

const updated = { id: 'u', released: '2026-10-01', first: '2026-01-01', releases: 3 };
const brandNew = { id: 'n', released: '2026-11-01', first: '2026-11-01', releases: 1 };
const older = { id: 'o', released: '2026-05-01', first: '2025-01-01', releases: 2 };

function byUpdated() {
  return new Function(`${line('wasUpdated')} ${line('byUpdated')} return byUpdated;`)();
}

test('Discover\'s Recently updated puts updated recomps before brand-new ones', () => {
  const order = [brandNew, older, updated].sort(byUpdated()).map((a) => a.id);
  assert.deepEqual(order, ['u', 'o', 'n']);
});

test('Home\'s Recently updated and Discover\'s sort use the same check', () => {
  assert.match(fn(app, 'recentlyUpdated'), /notPC\(a\) && wasUpdated\(a\)/);
});

function showOk({ follow = [], owned = {}, have = [] } = {}) {
  const run = new Function('S', 'follows', 'haveAny', 'greatHandheld', 'isSaved', 'needsSetup', 'setupDone',
    `${fn(app, 'showOk')} return showOk;`);
  return run({ library: {}, owned }, () => follow, (a) => have.includes(a.id), () => false, () => false, () => false, () => false);
}

test('Show: From developers you follow keeps only their recomps', () => {
  const ok = showOk({ follow: ['ana'] });
  assert.equal(ok({ id: 'a', dev: 'ana' }, 'follow'), true);
  assert.equal(ok({ id: 'b', dev: 'bob' }, 'follow'), false);
  assert.equal(ok({ id: 'c' }, 'follow'), false);
});

test('Show: Found in your folders leaves out games already in the Library', () => {
  const ok = showOk({ owned: { a: {}, b: {} }, have: ['b'] });
  assert.equal(ok({ id: 'a' }, 'owned'), true);
  assert.equal(ok({ id: 'b' }, 'owned'), false);
  assert.equal(ok({ id: 'c' }, 'owned'), false);
});

function showsNow({ follow = [], owned = {} } = {}) {
  const run = new Function('S', 'follows', 'N_', `${line('SHOWS')} ${line('showsNow')} ${app.match(/^const showHasAny = [\s\S]*?^};/m)[0]} return showsNow;`);
  return run({ owned }, () => follow, (s) => s);
}

test('Show offers Following and Found only when they can find something', () => {
  const keys = (list) => list.map(([k]) => k);
  assert.ok(!keys(showsNow()()).includes('follow'));
  assert.ok(!keys(showsNow()()).includes('owned'));
  assert.ok(keys(showsNow({ follow: ['ana'] })()).includes('follow'));
  assert.ok(keys(showsNow({ owned: { a: {} } })()).includes('owned'));
});

test('a Show choice already picked stays in the menu', () => {
  assert.ok(showsNow()('follow').some(([k]) => k === 'follow'));
});

test('Big Picture offers the same Show choices', () => {
  assert.match(tv, /sheet\(t\('Show'\), showsNow\(TV\.dshow \|\| ''\)/);
});

test('every shelf header ends in See all, then ‹ page count ›', () => {
  const body = app.slice(app.indexOf('function shelf('), app.indexOf('function layoutShelves('));
  assert.match(body, /<div class="pager">\$\{see\}<button type="button" class="ar"[^`]*?<\/button><small><\/small><button type="button" class="ar"/);
  assert.match(body, /<span class="see gap" aria-hidden="true">/);
});

test('the arrows and the page count show on a shelf that fits on one page', () => {
  const body = fn(app, 'layoutShelves');
  assert.doesNotMatch(body, /\.hidden = /);
  assert.match(body, /textContent = tf\('\{page\} of \{pages\}', \{ page: p \+ 1, pages \}\);/);
});

test('the page count keeps one width, so the arrows don\'t move while paging', () => {
  assert.match(css, /\.pager small\{[^}]*min-width:64px[^}]*font-variant-numeric:tabular-nums/);
  assert.match(css, /\.pager \.see\.gap\{visibility:hidden/);
});

function seeAll() {
  const S = { lib: { sel: null, filter: 'all', sort: 'az', origin: 'n64' }, lq: 'zel', q: 'mario', find: { sort: 'stars', genre: 'rpg', origin: 'ps2', tag: 'x', show: 'inst', easy: false, mp: true, great: true, deckgreat: true, picked: true, hideEnded: true } };
  const went = [];
  const block = app.match(/^const SEE_ALL = [\s\S]*?^};/m)[0];
  const run = new Function('S', 'go', `${block} ${line('FIND_CLEAR')} ${fn(app, 'seeAll')} return seeAll;`);
  return { S, went, see: run(S, (v) => went.push(v)) };
}

test('See all on Jump back in opens the Library, installed games, recently played', () => {
  const { S, went, see } = seeAll();
  see('played');
  assert.deepEqual(went, ['library']);
  assert.equal(S.lib.filter, 'inst');
  assert.equal(S.lib.sort, 'played');
  assert.equal(S.lib.origin, '');
  assert.equal(S.lq, '');
});

test('See all from the Library doesn\'t save the sort', () => {
  assert.doesNotMatch(fn(app, 'seeAll'), /saveUi/);
});

test('See all on Recently updated opens Discover with only that sort', () => {
  const { S, went, see } = seeAll();
  see('updated');
  assert.deepEqual(went, ['discover']);
  assert.equal(S.find.sort, 'updated');
  for (const k of ['genre', 'origin', 'tag', 'show']) assert.equal(S.find[k], '', k);
  for (const k of ['easy', 'mp', 'great', 'deckgreat', 'picked']) assert.equal(S.find[k], false, k);
  assert.equal(S.q, '');
});

test('See all keeps the player\'s own Hide Ended choice', () => {
  const { S, see } = seeAll();
  see('new');
  assert.equal(S.find.hideEnded, true);
});

test('See all on Ready to Play and Drop in Your Copy turn on Easy Setup', () => {
  const { S, see } = seeAll();
  see('ready');
  assert.equal(S.find.easy, true);
  assert.equal(S.find.sort, 'new');
  see('easy');
  assert.equal(S.find.easy, true);
  assert.equal(S.find.sort, 'popular');
});

test('See all on the followed and found shelves picks their Show choice', () => {
  const { S, see } = seeAll();
  see('follow');
  assert.equal(S.find.show, 'follow');
  see('found');
  assert.equal(S.find.show, 'owned');
});

test('every Home shelf has a See all, and each one has somewhere to go', () => {
  const home = fn(app, 'pageHome');
  const keys = [...home.matchAll(/shelf\('(\w+)'[^\n]*?see: true/g)].map((m) => m[1]);
  assert.deepEqual(keys.sort(), ['easy', 'favs', 'follow', 'new', 'played', 'popular', 'ready', 'updated']);
  const block = app.match(/^const SEE_ALL = [\s\S]*?^};/m)[0];
  for (const k of [...keys, 'found']) assert.match(block, new RegExp(`^  ${k}: \{`, 'm'), k);
  assert.match(fn(app, 'foundSection'), /\$\{seeAllBtn\('found'\)\}/);
});
