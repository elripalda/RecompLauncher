const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const escape = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function context(settings, hoard) {
  const c = vm.createContext({
    S: { os: 'windows', settings: Object.assign({ library_dir: 'D:\\Games', ui: {} }, settings), hoard },
    t: (s) => s, esc: escape,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    tn: (n, one, many) => (n === 1 ? one : many).replace('{n}', n),
    ago: () => '2 minutes ago', cleanMsg: (m) => m,
    invoke: () => Promise.resolve(), $: () => null,
  });
  // The settings page's own helpers: row() escapes what it is given, as the real one does.
  c.group = (title, body) => `<h3>${title}</h3>${body}`;
  c.row = (b, s, ctl) => `<div class="set-row"><b>${escape(b)}</b><span>${escape(s)}</span>${ctl}</div>`;
  c.sw = (k, value, label) => `<button aria-checked="${!!value}" aria-label="${label}" data-act="toggle" data-k="${k}"></button>`;
  vm.runInContext(['hoardRoot', 'hoardLine', 'paintHoard', 'hoarderGroup'].map((n) => fn(app, n)).join('\n'), c);
  return c;
}

const html = (c) => c.hoarderGroup(c.group, c.row, c.sw, c.S.settings);

test('off, only the switch shows', () => {
  const out = html(context({ hoarder: false }));
  assert.match(out, /data-k="hoarder"/);
  assert.doesNotMatch(out, /Save Now/);
});

test('on, the status line and folder are real markup, not escaped text', () => {
  const out = html(context({ hoarder: true }, { running: true, game: 'Game', release: 'v1', file: 'a.zip', done: 59, total: 100 }));
  assert.doesNotMatch(out, /&lt;span|&lt;b/);
  assert.match(out, /<span id="hoard-line">Saving Game v1: a\.zip 59%<\/span>/);
  assert.match(out, /<b class="path">D:\\Games\\Releases<\/b>/);
});

test('the folder follows the picked one, and Reset only shows for a picked one', () => {
  assert.doesNotMatch(html(context({ hoarder: true })), /hoard-dir-reset/);
  const out = html(context({ hoarder: true, hoard_dir: 'E:\\Vault' }));
  assert.match(out, /E:\\Vault/);
  assert.match(out, /hoard-dir-reset/);
});

test('the keep choice shows a number box only for "latest releases, a number"', () => {
  assert.doesNotMatch(html(context({ hoarder: true, hoard_keep: 'latest' })), /id="hoard-count"/);
  const out = html(context({ hoarder: true, hoard_keep: 'count', hoard_count: 8 }));
  assert.match(out, /id="hoard-count"[^>]*value="8"/);
  assert.match(out, /<option value="count" selected>/);
});
