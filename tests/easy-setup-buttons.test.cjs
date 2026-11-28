const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const actions = app.match(/^function actions\([^]*?^}/m)[0];
// Big Picture's main button on a game page.
const tvButton = tv.match(/case 'installed': \{ if \(setupDue[^\n]*\n[^]*?default: return isSaved\(id\)[^\n]*/)[0];

test('the installed Easy Setup button is green like the one before install', () => {
  assert.match(actions, /easyNotReady\(a\.id\) \? b\('main easy', 'setup', esc\(t\('Easy Setup'\)\)/);
  assert.match(tvButton, /btn\(key, 'setup', esc\(t\('Easy Setup'\)\), \{ cls: 'main easy'/);
});

test('the install buttons leave the size to the game page', () => {
  assert.doesNotMatch(actions, /a\.size/);
  assert.doesNotMatch(tvButton, /a\.size/);
});

test('Locate Installed is a square open-folder button with its name in the tooltip', () => {
  assert.match(actions, /<button type="button" class="\$\{sq\}" data-act="have-it" data-id="\$\{id\}" aria-label="\$\{esc\(t\('Locate Installed'\)\)\}" title="\$\{esc\(t\('Locate Installed'\)\)\}">\$\{FOLDER_OPEN\}<\/button>/);
});

test('Big Picture keeps Locate Installed as a named button, with the same open folder', () => {
  assert.match(tv, /btn\('g-have', 'haveit', esc\(t\('Locate Installed'\)\), \{ id, icon: 'openfolder' \}\)/);
});
