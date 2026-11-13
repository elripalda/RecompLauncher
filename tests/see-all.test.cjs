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
