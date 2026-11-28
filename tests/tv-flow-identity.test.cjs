const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

// Exercise the caption written when focus moves to a cover in Big Picture's Library.
function focusedCaption(app, developer = null) {
  const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
  const start = tv.indexOf('  function flowLayout(el, instant) {');
  const end = tv.indexOf('\n  // Pictures fetched ahead', start);
  assert.ok(start >= 0 && end > start);
  const cap = { innerHTML: '', classList: { remove() {}, add() {} }, offsetWidth: 1 };
  const row = {};
  const el = { closest: () => row, dataset: { hero: app.id } };
  const FLOW = { row, items: [el], pos: 0, target: 0 };
  const S = { library: { [app.id]: {} } };
  const fn = new Function('FLOW', 'S', 'TV', 'root', '$', 'libAppOf', 'timeLine', 'devOf', 'tf', 'esc', 'lighter', 'flowPaint', 'preload', 'heroOf', 'tiltStart', `${tv.slice(start, end)}; return flowLayout;`);
  const esc = (s) => String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
  const flowLayout = fn(FLOW, S, {}, {}, () => cap, () => app, () => 'Not played yet', () => developer, (_, v) => `By ${v.who}`, esc, () => true, () => {}, () => {}, () => '', () => {});
  flowLayout(el, true);
  return cap.innerHTML;
}

test('Cover Flow identifies the project before the original game', () => {
  const html = focusedCaption({ id: 'savagewind', name: 'SavageWind', game: 'The Legend of Zelda' }, { name: 'Revi' });
  assert.match(html, /^<b>SavageWind<\/b>/);
  assert.match(html, /The Legend of Zelda/);
  assert.match(html, /By Revi/);
  assert.match(html, /Not played yet/);
});

test('Cover Flow falls back to the author and escapes names when no game is known', () => {
  const html = focusedCaption({ id: 'custom', name: 'A & B', author: '<Studio>' });
  assert.equal(html, '<b>A &amp; B</b><small>By &lt;Studio&gt; · Not played yet</small>');
});

test('Cover Flow does not repeat an identical project and game name', () => {
  const html = focusedCaption({ id: 'same', name: 'Same', game: 'Same' });
  assert.equal(html, '<b>Same</b><small>Not played yet</small>');
});
