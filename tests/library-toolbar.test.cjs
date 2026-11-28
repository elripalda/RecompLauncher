const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

// A function declaration out of app.js, by name.
function fn(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  let i = app.indexOf('{', start), depth = 0;
  for (; i < app.length; i++) {
    if (app[i] === '{') depth++;
    if (app[i] === '}' && --depth === 0) break;
  }
  return app.slice(start, i + 1);
}

// The library's toolbar, from the search box to the end of its row of controls.
function toolbar() {
  const page = fn('pageLibrary');
  const start = page.indexOf('<div class="dctl">');
  assert.ok(start >= 0, 'dctl');
  return page.slice(start, page.indexOf('</div></div>', start));
}

test('the Library toolbar leaves Bring Your Recomps to Settings', () => {
  assert.doesNotMatch(toolbar(), /data-act="adopt"/);
});

test('Settings still has a Bring Your Games section', () => {
  assert.match(app, /\['bring', N_\('Bring Your Games'\)\]/);
});

test('an empty Library still offers Bring Your Recomps', () => {
  const page = fn('pageLibrary');
  const empty = page.slice(0, page.indexOf('<div class="dctl">'));
  assert.match(empty, /data-act="adopt">\$\{esc\(t\('Bring Your Recomps'\)\)\}/);
});
