// Settings → Networking: Hide the Site's Name (ech.rs), the same on the desktop and in Big Picture.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js');
const appFn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];
const tvFn = (name) => tv.match(new RegExp(`^  (?:async )?function ${name}\\([^]*?^  }`, 'm'))[0];
const consts = ['HIDE_NAME', 'HIDE_NAME_NOTE'].map((k) => app.match(new RegExp(`^const ${k} = .*$`, 'm'))[0]).join('\n');

function context(hideName) {
  const c = vm.createContext({
    S: { settings: { hide_name: hideName } },
    N_: (x) => x,
    t: (x) => x,
    esc: (x) => String(x ?? '').replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch])),
  });
  vm.runInContext([consts, appFn('hideNameRow'), tvFn('hideNameItem')].join('\n'), c);
  return c;
}

test('Networking sits in Settings right after Updates and downloads', () => {
  const sections = app.match(/^const SET_SECTIONS = (.*);$/m)[1];
  assert.match(sections, /\['updates', N_\('Updates and downloads'\)\], \['network', N_\('Networking'\)\]/);
  assert.match(app, /case 'network':\s+body = networkGroup\(group, row, st\);/);
});

test('the desktop offers Automatic, Always and Never, Automatic first', () => {
  const html = context(undefined).hideNameRow();
  const choices = [...html.matchAll(/data-k="(\w*)">(\w+)</g)].map((m) => [m[1], m[2]]);
  assert.deepEqual(choices, [['', 'Automatic'], ['always', 'Always'], ['never', 'Never']]);
  assert.match(html, /aria-checked="true" data-act="hide-name" data-k="">Automatic/);
});

test('the explanation says Cloudflare and Google can be asked', () => {
  const html = context('').hideNameRow();
  assert.match(html, /Cloudflare and Google/);
});

test('the saved choice is the one marked', () => {
  const html = context('never').hideNameRow();
  assert.match(html, /aria-checked="true" data-act="hide-name" data-k="never"/);
  assert.match(html, /aria-checked="false" data-act="hide-name" data-k=""/);
});

test('Big Picture shows the same choices and the same explanation', () => {
  const c = context('always');
  const item = c.hideNameItem();
  assert.match(item, /data-tvact="hidename" data-seg="1" data-val="Always"/);
  assert.match(item, /<span class="">Automatic<\/span><span class="on">Always<\/span><span class="">Never<\/span>/);
  assert.match(item, /Cloudflare and Google/);
  assert.match(tv, /\$\{head\(t\('Networking'\)\)\}\s+\$\{hideNameItem\(\)\}/);
});

test('Big Picture saves the choice it rolls to', () => {
  assert.match(tv, /act === 'hidename'\) await saveSettings\(\{ hide_name: roll\(HIDE_NAME\.map\(\(\[k\]\) => k\), S\.settings\.hide_name \|\| ''\) \}, true\)/);
});
