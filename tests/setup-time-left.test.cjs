const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js');

// stepLeft out of app.js, with took and a tf that fills in its values.
function load() {
  const start = app.indexOf('const took = ');
  const end = app.indexOf('// (0.91) Each download');
  assert.ok(start >= 0 && end > start);
  const tf = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => v[k]);
  return new Function('tf', `${app.slice(start, end)} return stepLeft;`)(tf);
}
const ago = (sec) => Date.now() - sec * 1000;

test('half a step in 30 seconds leaves 30 seconds', () => {
  assert.equal(load()({ pct: 0.5, from: { at: ago(30), pct: 0 } }), '30 sec left');
});

test('a long step says minutes', () => {
  assert.equal(load()({ pct: 0.25, from: { at: ago(60), pct: 0 } }), '3 min left');
});

test('the rate counts from where the step was when it first showed', () => {
  assert.equal(load()({ pct: 0.6, from: { at: ago(10), pct: 0.4 } }), '20 sec left');
});

test('nothing is said in the first seconds', () => {
  assert.equal(load()({ pct: 0.5, from: { at: ago(2), pct: 0 } }), '');
});

test('a step that sends no progress says nothing', () => {
  assert.equal(load()({ from: { at: ago(60), pct: 0 } }), '');
  assert.equal(load()({ pct: 0, from: { at: ago(60), pct: 0 } }), '');
});

test('a finished step says nothing', () => {
  assert.equal(load()({ pct: 1, from: { at: ago(60), pct: 0 } }), '');
});

test('the desktop setup window has Run in Background while it runs', () => {
  assert.match(app, /<button type="button" class="btn big" data-su="bg">\$\{esc\(t\('Run in Background'\)\)\}<\/button>/);
  assert.match(app, /case 'bg': return suBackground\(\);/);
});

test('desktop and Big Picture both put the time left on the running step', () => {
  assert.match(app, /const left = stepLeft\(st\[SU\.step\]\);/);
  assert.match(tv, /const left = stepLeft\(s\[TS\.step\]\);/);
});
