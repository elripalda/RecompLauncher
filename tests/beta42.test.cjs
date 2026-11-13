const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), adopt = read('adopt.js'), first = read('firstrun.js');

// A function declaration out of adopt.js, by name.
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

// The three boxes, worked out from what a scan found.
function groups(list, { library = {}, savedIds = [], catalog = [] } = {}) {
  const run = new Function('MIG', 'S', 'isSaved', 'all', 'adoptKey',
    `${fn(adopt, 'migKnownLink')} ${fn(adopt, 'migGroups')} return migGroups();`);
  return run({ list }, { library }, (id) => savedIds.includes(id), () => catalog, (f) => String(f.path).toLowerCase());
}

test('sure matches are ready, the rest need a look, and list-only games wait to be added', () => {
  const g = groups([
    { id: 'a', sure: true, path: '/g/a' },
    { id: 'b', sure: false, path: '/g/b', guesses: ['b'] },
    { id: '', sure: false, path: '/g/c' },
    { id: 'd', sure: true, missing: true, path: '/g/d' },
    { id: '', missing: true, link: 'https://github.com/x/e', path: '/g/e' },
    { id: 'f', sure: true, path: '/g/f', have: true },
  ]);
  assert.deepEqual(g.ready.map((f) => f.path), ['/g/a']);
  assert.deepEqual(g.look.map((f) => f.path), ['/g/b', '/g/c']);
  assert.deepEqual(g.away.map((f) => f.path), ['/g/d', '/g/e']);
  assert.equal(g.have, 1);
});

test('a list-only game already saved, installed or added from its link isn\'t offered again', () => {
  const g = groups([
    { id: 'd', sure: true, missing: true, path: '/g/d' },
    { id: 'i', sure: true, missing: true, path: '/g/i' },
    { id: '', missing: true, link: 'https://github.com/X/E/', path: '/g/e' },
    { id: '', missing: true, link: 'https://gitlab.com/grp/p', path: '/g/p' },
  ], { savedIds: ['d'], library: { i: {} }, catalog: [{ repo: 'x/e' }, { repo: 'gitlab:grp/p' }] });
  assert.equal(g.away.length, 0);
});

test('Bring Your Games is its own Settings tab with three boxes the same size', () => {
  assert.match(app, /\['bring', N_\('Bring Your Games'\)\]/);
  assert.match(app, /case 'bring':\n\s*body = window\.migratePage \? window\.migratePage\(group\) : '';/);
  assert.match(app, /if \(\/\^mig-\/\.test\(a\) && window\.migrateAct\) return window\.migrateAct\(a, el\);/);
  assert.equal((adopt.match(/\$\{box\(/g) || []).length, 3);
  assert.match(read('app.css'), /\.mig-sum\{display:grid;grid-template-columns:repeat\(3,minmax\(0,1fr\)\)/);
});

test('nothing unsure is added until the player picks which game it is', () => {
  const look = fn(adopt, 'migLook');
  assert.match(look, /if \(w === 'pick'\) added \+= await migAdopt\(f, x\.dataset\.id\);/);
  assert.doesNotMatch(look, /adopt_add/);
  assert.match(fn(adopt, 'migAddReady'), /migGroups\(\)\.ready/);
});

test('the launcher\'s own words don\'t name other launchers', () => {
  for (const [f, src] of [['app.js', app], ['adopt.js', adopt], ['firstrun.js', first]]) assert.doesNotMatch(src, /quiver/i, f);
});
