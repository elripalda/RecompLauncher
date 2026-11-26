const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const css = fs.readFileSync(path.join(__dirname, '../ui/app.css'), 'utf8');
const lightTokens = () => css.match(/^body\.light\{--noise[^}]*\}/m)[0];

test('a switch takes its off colours from theme values', () => {
  assert.match(css, /^\.switch\{[^}]*background:var\(--switch-off\)/m);
  assert.match(css, /^\.switch::after\{[^}]*background:var\(--switch-knob\)/m);
});

test('the light theme sets the switch values', () => {
  assert.match(lightTokens(), /--switch-off:/);
  assert.match(lightTokens(), /--switch-knob:/);
});

// "body.light .switch" outranks ".switch[aria-checked=true]", so a switch that is on looked off.
test('no theme rule restyles a switch directly', () => {
  assert.doesNotMatch(css, /body\.light \.switch(::after)?\{/);
});
