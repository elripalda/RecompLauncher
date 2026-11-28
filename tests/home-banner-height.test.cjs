const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const css = fs.readFileSync(path.join(__dirname, '../ui/app.css'), 'utf8');
const sliderRule = () => css.match(/^\.slider\{position:relative;[^}]*\}/m)[0];
const shortWindow = () => css.match(/^@media \(max-height:860px\)\{.*\}$/m)[0];

test('the Featured Banner never takes more than half the window\'s height', () => {
  assert.match(sliderRule(), /height:clamp\(260px,min\(40vw,50vh\),520px\)/);
});

test('on a short window the banner\'s logo is smaller', () => {
  assert.match(shortWindow(), /\.sl-logo\{height:80px\}/);
});

test('on a short window the banner\'s cover follows the window\'s height', () => {
  assert.match(shortWindow(), /\.sl-cover\{[^}]*width:clamp\(110px,21vh,210px\)/);
});

test('on a very short window the banner leaves out its blurb', () => {
  assert.match(css, /^@media \(max-height:640px\)\{\.sl-in p\{display:none\}\}$/m);
});
