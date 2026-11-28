const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const css = fs.readFileSync(path.join(__dirname, '../ui/app.css'), 'utf8');

function rule(selector) {
  const start = css.indexOf(`\n${selector}{`);
  assert.ok(start >= 0, selector);
  return css.slice(start, css.indexOf('}', start));
}

// #454: some WebView2 builds don't stretch a flex button's children, so a poster without its own width drew 0 x 0.
test('the tile button stretches its poster', () => {
  assert.match(rule('.tile-b'), /align-items:stretch/);
});

test('the poster sets its own width', () => {
  assert.match(rule('.poster'), /[{;]width:100%/);
});
