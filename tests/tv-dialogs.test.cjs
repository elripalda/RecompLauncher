const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const tvCss = read('tv.css'), tvJs = read('tv.js'), appCss = read('app.css');

test('the island that holds the messages is centred on the whole window, in TV mode too', () => {
  assert.match(appCss, /\.island\{position:fixed;left:50%;/);
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

test('the way out on Home offers Quit RecompLauncher', () => {
  assert.match(tvJs, /label: t\('Quit RecompLauncher'\), icon: 'close', run: \(\) => invoke\('quit_launcher'\)/);
});

test('the quit command is registered with the window', () => {
  const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
  assert.match(lib, /tray::quit_launcher/);
});

test('Big Picture picks a build with the controller, in a sheet of its own', () => {
  assert.match(tvJs, /window\.tvPickBuild = \(id, builds, current, go\) => \{/);
});

test('every Big Picture hook the desktop asks for is there in tv.js', () => {
  const appJs = read('app.js');
  const asked = new Set([...appJs.matchAll(/window\.(tv[A-Z]\w*)/g)].map((m) => m[1]));
  const missing = [...asked].filter((n) => !new RegExp(`window\\.${n} *=`).test(tvJs));
  assert.deepEqual(missing, []);
});
