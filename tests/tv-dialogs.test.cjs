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

test('up and down on the controller scroll an open dialog\'s long text', () => {
  assert.match(tvJs, /if \(dir && dialogReads\(dlg, dir\)\) return dialogScroll\(dlg, dir\);/);
});

test('the d-pad moves between a desktop dialog\'s choices in Big Picture', () => {
  assert.match(tvJs, /window\.deskPad\(name\);/);
  assert.match(tvJs, /const dir = \{ up: -1, down: 1, left: 0, right: 0 \}\[name\];/);
});

test('A on a dialog with nothing lit still presses its main button at once', () => {
  assert.match(tvJs, /if \(!lit\) \{\n {6}window\.deskPad\('down'\);\n {6}if \(name !== 'a'\) return;/);
});

test('the controller answers the newest dialog when one opened another', () => {
  assert.match(tvJs, /const dlg = \[\.\.\.document\.querySelectorAll\('\.scrim'\)\]\.pop\(\);/);
});

test('the way out on Home offers Quit RecompLauncher', () => {
  assert.match(tvJs, /label: t\('Quit RecompLauncher'\), icon: 'close', run: \(\) => invoke\('quit_launcher'\)/);
});

test('the quit command is registered with the window', () => {
  const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
  assert.match(lib, /tray::quit_launcher/);
});
