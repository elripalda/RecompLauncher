const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// The Manage tab's start rows for one installed game, with the rows around Run Setup Again stubbed.
const rows = (inst) => {
  const run = new Function('S', 't', 'tf', 'esc', 'baseName', 'easyRow', 'programRow', `${fn(app, 'manageStartRows')}; return manageStartRows;`);
  const tf = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
  const baseName = (p) => String(p).split(/[\\/]/).filter(Boolean).pop();
  const S = { os: 'windows', setup: {}, easy: new Set() };
  return run(S, (x) => x, tf, (x) => x, baseName, () => '', () => '')('g', { path: 'C:\\Recomp\\hookshot', args: '', ...inst }).join('');
};

test('a game with its own setup can open it again from the Manage tab', () => {
  const html = rows({ setup: 'HookshotBuilder.exe' });
  assert.match(html, /<b>Run Setup Again<\/b><span>Opens HookshotBuilder\.exe, the program that sets this game up or builds it\.<\/span>/);
  assert.match(html, /data-act="own-setup" data-id="g">Open</);
});

test('the setup is named by its file, even in a folder inside the game', () => {
  assert.match(rows({ setup: 'tools\\Setup.exe' }), /Opens Setup\.exe,/);
});

test('a game without a setup of its own has no Run Setup Again', () => {
  assert.doesNotMatch(rows({ setup: null }), /own-setup|Run Setup Again/);
});
