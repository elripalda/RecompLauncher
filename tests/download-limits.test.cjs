const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// Settings → Downloads → Downloads at Once and Unpacks at Once (this fork's limits).
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const hoard = fs.readFileSync(path.join(__dirname, '../src-tauri/src/hoard.rs'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const line = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];

function context(ui) {
  const c = vm.createContext({ S: { settings: { ui } }, t: (s) => s, esc: String, tf: (s) => s });
  c.row = (b, s, ctl) => `<row ${b}>${ctl}</row>`;
  vm.runInContext(['DL_AT_ONCE', 'UNPACK_AT_ONCE', 'dlAtOnce', 'unpackAtOnce'].map(line).join('\n'), c);
  vm.runInContext(['atOnceSel', 'atOnceRows', 'sendLimits', 'progLabel'].map((n) => fn(app, n)).join('\n'), c);
  return c;
}

test('unset, one download at a time and unpacks unlimited, as the launcher always did', () => {
  const html = context({}).atOnceRows((b, s, ctl) => `<row ${b}>${ctl}</row>`, {});
  assert.match(html, /data-act="dl-max"[^]*<option value="1" selected>1<\/option>/);
  assert.match(html, /data-act="unpack-max"[^]*<option value="0" selected>No Limit<\/option>/);
});

test('the picked limits are shown and sent to the app', async () => {
  const ui = { dl_max: 3, unpack_max: 2, dl_limit: 5 };
  const c = context(ui);
  const html = c.atOnceRows(c.row, ui);
  assert.match(html, /data-act="dl-max"[^]*<option value="3" selected>3<\/option>/);
  assert.match(html, /data-act="unpack-max"[^]*<option value="2" selected>2<\/option>/);
  let sent;
  c.invoke = async (command, args) => { sent = { command, args }; };
  await c.sendLimits();
  assert.deepEqual(JSON.parse(JSON.stringify(sent)), { command: 'download_limits', args: { mb: 5, slowPlaying: true, downloads: 3, unpacks: 2 } });
});

test('No Limit for downloads is kept as 0, not put back to one', () => {
  const c = context({ dl_max: 0 });
  assert.equal(vm.runInContext('dlAtOnce({ dl_max: 0 })', c), 0);
  assert.equal(vm.runInContext('dlAtOnce({})', c), 1);
});

test('a finished download waiting for an unpack slot says so', () => {
  const c = context({});
  assert.equal(c.progLabel({ phase: 'unpack-wait' })[0], 'Waiting to unpack');
});

test('installs, updates and Data Hoarder saves all wait on the same download slots', () => {
  assert.match(lib, /let permit = match slots::DOWNLOADS\.try_take\(\)/);
  assert.match(hoard, /crate::slots::DOWNLOADS\.take\(&stop_flag\(\)\)\.await/);
  assert.equal((lib.match(/let _unpacking = unpack_turn\(id, &cancel, &report\)\.await\?;/g) || []).length, 2);
});
