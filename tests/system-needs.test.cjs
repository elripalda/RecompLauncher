// What a game needs from the PC (`system` in its recomplauncher.json): the game page's rows, which
// the desktop and Big Picture share, and what's said when it starts on an older PC.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js'), tv = read('ui/tv.js');
const fn = (src, name) => src.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];
const tf = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
const names = app.match(/^const GRAPHICS_NAMES = [^\n]*$/m)[0];
const rowsFor = (os) => new Function('S', 't', 'tf', `${names}; ${fn(app, 'systemRows')}; return systemRows;`)({ os }, (s) => s, tf);
const oldText = new Function('tf', `${fn(app, 'systemOldText')}; return systemOldText;`)(tf);

test('an old glibc shows in red with what this PC has', () => {
  const su = { system: { glibc: '2.35' }, system_old: [{ what: 'glibc', want: '2.35', have: '2.31' }] };
  const [row] = rowsFor('linux')(su, { glibc: '2.31' });
  assert.equal(row.k, 'glibc 2.35 or newer');
  assert.equal(row.v, false);
  assert.equal(row.d, 'This PC has 2.31. The game may not start.');
});

test('a new enough Windows shows as met', () => {
  const su = { system: { version: '10' }, system_old: [] };
  const [row] = rowsFor('windows')(su, { os_version: '10.0.22631' });
  assert.equal(row.k, 'Windows 10 or newer');
  assert.equal(row.v, true);
  assert.equal(row.d, 'This PC has 10.0.22631');
});

test('a version the PC cannot tell is neither met nor missed', () => {
  const [row] = rowsFor('macos')({ system: { version: '13' }, system_old: [] }, {});
  assert.equal(row.v, null);
  assert.equal(row.d, '');
});

test('the graphics API is shown by its name, never checked', () => {
  const rows = rowsFor('linux')({ system: { graphics: { vulkan: '1.3' } } }, {});
  assert.equal(rows.length, 1);
  assert.equal(rows[0].k, 'Graphics: Vulkan 1.3');
  assert.equal(rows[0].v, null);
});

test('a Windows build through Proton on Linux shows no Windows version row', () => {
  assert.deepEqual(rowsFor('linux')({ system: { version: '11' } }, {}), []);
});

test('no system part, no rows', () => {
  assert.deepEqual(rowsFor('linux')({}, {}), []);
  assert.deepEqual(rowsFor('linux')(undefined, {}), []);
});

test('starting on an older PC says what it needs and what the PC has', () => {
  assert.equal(oldText('Hopper', { what: 'glibc', want: '2.35', have: '2.31' }), 'Hopper needs glibc 2.35 or newer, and this PC has 2.31. It may not start.');
  assert.equal(oldText('Hopper', { what: 'macos', want: '14', have: '13.6' }), 'Hopper needs macOS 14 or newer, and this PC has 13.6. It may not start.');
});

test('the rows are in the list both views show, and the start message has one listener', () => {
  assert.match(fn(app, 'readyRows'), /rows\.push\(\.\.\.systemRows\(su, pc\)\);/);
  assert.match(tv, /const checks = readyRows\(a\);/);
  assert.match(app, /T\.event\.listen\('system-old', /);
  assert.doesNotMatch(tv, /listen\('system-old'/);
});

test('every new text is in every language', () => {
  const texts = ['{what} or newer', 'This PC has {version}', 'This PC has {version}. The game may not start.', 'Graphics: {api} {version}',
    'Your graphics card and its driver need to support it', '{name} needs {what} or newer, and this PC has {have}. It may not start.'];
  const dir = path.join(__dirname, '../ui/i18n');
  // en-GB and fr-CA keep only what they say differently from English and French.
  for (const f of fs.readdirSync(dir).filter((x) => x.endsWith('.json') && x !== 'en-GB.json' && x !== 'fr-CA.json')) {
    const d = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    for (const s of texts) assert.ok(d[s], `${f}: ${s}`);
  }
});
