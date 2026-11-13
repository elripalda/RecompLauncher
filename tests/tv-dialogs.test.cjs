const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const tvCss = read('tv.css'), tvJs = read('tv.js'), appCss = read('app.css');

test('TV mode centres the island that holds the messages', () => {
  assert.match(appCss, /body\.tv-on \.island\{left:50%\}/);
});

test('TV mode does not shift the messages again inside the centred island', () => {
  assert.doesNotMatch(tvCss, /body\.tv-on \.toasts\{[^}]*transform/);
});

test('What\'s New gets a TV-sized window in TV mode', () => {
  assert.match(tvCss, /body\.tv-on \.modal:has\(\.whatsnew\)\{width:min\(1040px,88vw\)\}/);
});

test('up and down on the controller scroll an open dialog', () => {
  assert.match(tvJs, /if \(name === 'up' \|\| name === 'down'\) \{ dialogScroll\(dlg/);
});
