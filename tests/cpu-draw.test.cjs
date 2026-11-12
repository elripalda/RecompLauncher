const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const read = (file) => fs.readFileSync(path.join(__dirname, '..', file), 'utf8');
const appJs = read('ui/app.js');
const appCss = read('ui/app.css');
const tvCss = read('ui/tv.css');

// When the web view draws on the processor, everything that keeps moving costs a core.
test('the page marks itself when the core says it draws on the processor', () => {
  assert.match(appJs, /classList\.toggle\('cpu-draw', !!st\.cpuDraw\)/);
});

test('the hero picture stands still when drawn on the processor', () => {
  assert.match(appCss, /body\.cpu-draw \.slide>\.bg\{transform:none;transition:none\}/);
});

test('a new hero slide does not fly in when drawn on the processor', () => {
  assert.match(appCss, /body\.cpu-draw \.slide\.on \.sl-in>\*,body\.cpu-draw \.slide\.on \.sl-cover\{animation:none\}/);
});

test('the Big Picture backdrop stands still when drawn on the processor', () => {
  assert.match(tvCss, /body\.cpu-draw \.tv-ambient i\{animation:none!important\}/);
});

test('the Big Picture arrow stops bobbing when drawn on the processor', () => {
  assert.match(tvCss, /body\.cpu-draw \.tv-down\{animation:none\}/);
});
