const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), cz = read('customize.js');

// Settings > Game Art's LaunchBox row, out of app.js, for a PC on `os`.
function row(os, ui) {
  const start = app.indexOf('// Settings > Game Art: the player\'s own LaunchBox folder');
  const end = app.indexOf('// A LaunchBox folder checked and kept');
  assert.ok(start >= 0 && end > start);
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const f = new Function('S', 't', 'esc', `${app.slice(start, end)} return launchboxRow;`);
  return f({ os }, (s) => s, esc)(ui);
}

test('the LaunchBox row shows on Windows', () => {
  assert.match(row('windows', {}), /LaunchBox Folder/);
});

test('the LaunchBox row stays away on Linux until a folder is set', () => {
  assert.equal(row('linux', {}), '');
  assert.match(row('linux', { launchbox_dir: '/mnt/games/LaunchBox' }), /\/mnt\/games\/LaunchBox/);
});

test('a set folder can be changed or removed, not found again', () => {
  const html = row('windows', { launchbox_dir: 'C:\\LaunchBox' });
  assert.match(html, /data-act="lb-pick"/);
  assert.match(html, /data-act="lb-forget"/);
  assert.doesNotMatch(html, /data-act="lb-find"/);
});

test('Customize offers LaunchBox pictures through the same frame as a file', () => {
  assert.match(cz, /case 'lb-take':[^\n]*take\(spec, \{ path: pic\.path \}\)/);
});

test('the LaunchBox commands are registered with the core', () => {
  const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
  for (const c of ['launchbox_find', 'launchbox_search', 'launchbox_pictures']) assert.ok(lib.includes('launchbox::' + c), c);
});
