const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');

// i18n.js on its own, as a page loads it: what it puts on window comes back.
function i18n() {
  const window = {};
  vm.runInNewContext(read('i18n.js'), { window, navigator: { languages: ['en-US'] }, document: {} });
  return window;
}
const afternoon = new Date(2000, 0, 1, 15, 30);
const show = (w) => afternoon.toLocaleTimeString(w.curLocale(), w.timeOpts({ hour: 'numeric', minute: '2-digit' }));

test('left on auto, English (US) keeps its 12-hour clock', () => {
  const w = i18n();
  assert.match(show(w), /^3:30\sPM$/);
});

test('the 24-hour clock shows English (US) times as 15:30', () => {
  const w = i18n();
  w.setClock('24');
  assert.equal(show(w), '15:30');
});

test('the 24-hour clock shows midnight as 00, not 24', () => {
  const w = i18n();
  w.setClock('24');
  const midnight = new Date(2000, 0, 1, 0, 5).toLocaleTimeString('en', w.timeOpts({ hour: '2-digit', minute: '2-digit' }));
  assert.equal(midnight, '00:05');
});

test('the 12-hour clock works in a language that uses 24 hours', () => {
  const w = i18n();
  w.setClock('12');
  const de = afternoon.toLocaleTimeString('de', w.timeOpts({ hour: 'numeric', minute: '2-digit' }));
  assert.match(de, /^3:30/);
});

test('an unknown clock value goes back to auto', () => {
  const w = i18n();
  w.setClock('24');
  w.setClock('7');
  assert.equal(JSON.stringify(w.timeOpts({ hour: 'numeric' })), '{"hour":"numeric"}');
});

test('every time on screen goes through timeOpts', () => {
  for (const f of ['app.js', 'tv.js', 'overlay.js', 'account.js', 'achievements.js']) {
    const lines = read(f).split('\n').filter((l) => /toLocaleTimeString|hour: '(numeric|2-digit)'/.test(l));
    for (const l of lines) assert.ok(/timeOpts|clockSample/.test(l), `${f}: ${l.trim().slice(0, 120)}`);
  }
});

test('the overlay reads the clock with the language', () => {
  assert.match(read('overlay.js'), /setClock\(ui\.clock\)/);
});
