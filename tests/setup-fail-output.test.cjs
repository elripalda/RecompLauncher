const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const test = require('node:test');
const { runInNewContext } = require('node:vm');

// A setup step that stopped: the player sees the launcher's line (translated, with the step's
// code), the last lines the core sent, then Copy (for the developer) and Open Full Log
// (everything it printed, in a file). Desktop and Big Picture draw it with the same helpers.
const ui = (f) => readFileSync(join(__dirname, '../ui', f), 'utf8');
const app = ui('app.js');
const tv = ui('tv.js');
const between = (src, from, to) => src.slice(src.indexOf(from), src.indexOf(to));
const helpersSrc = between(app, '// A setup that stopped', '// Steps that finish quickly still show');

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const tf = (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]);

function helpers({ library = {}, calls = [] } = {}) {
  const S = { library, os: 'linux', version: '0.90.0-beta.68' };
  const context = {
    t: String, tf, esc, S,
    OS_NAME: { windows: 'Windows', macos: 'Mac', linux: 'Linux' },
    byId: () => null,
    parentId: (id) => id,
    toast: (m) => calls.push(['toast', m]),
    invoke: async (cmd, args) => { calls.push([cmd, args]); },
    navigator: { clipboard: { writeText: async (s) => { calls.push(['clip', s]); } } },
  };
  return runInNewContext(`${helpersSrc}\n({ setupFail, setupFailHtml, setupFailTitle, setupFailShort, setupFailDetails, setupFailAct, SU_FAIL })`, context);
}

const DEBIAN = [
  'Its setup step stopped with code 1.',
  '[missing] Vulkan SDK (libvulkan-dev)',
  'Missing packages. Install them with:',
  '  sudo apt install cmake ninja-build',
  '  libvulkan-dev vulkan-validation-layers spirv-tools dotnet-sdk-10.0 git',
].join('\n');

const button = (act, label) => `<b data-act="${act}">${label}</b>`;
const opts = { p: 'err', row: 'row', tail: 'tail', button };

test('the launcher line of a failed setup step is read with its code', () => {
  const f = helpers().setupFail(DEBIAN);
  assert.equal(f.head, 'Its setup step stopped with code 1.');
  assert.equal(f.step, true);
});

test('the lines the step printed are kept apart from the launcher line', () => {
  assert.equal(helpers().setupFail(DEBIAN).printed.length, 4);
});

test('the error shows the reason first', () => {
  const html = helpers().setupFailHtml(DEBIAN, opts);
  assert.ok(html.startsWith('<p class="err">Its setup step stopped with code 1.</p>'));
});

test('the error always shows every line the core sent', () => {
  const html = helpers().setupFailHtml(DEBIAN, opts);
  const box = html.match(/<pre class="tail">([\s\S]*)<\/pre>/)[1];
  assert.equal(box.split('\n').length, 4);
});

test('Copy and Open Full Log come under the log', () => {
  const html = helpers().setupFailHtml(DEBIAN, opts);
  assert.deepEqual([...html.matchAll(/data-act="(\w+)"/g)].map((m) => m[1]), ['copy', 'open']);
  assert.ok(html.indexOf('</pre>') < html.indexOf('data-act'));
});

test('a step that printed nothing shows only the reason', () => {
  const html = helpers().setupFailHtml('Its setup step stopped with code 2.', opts);
  assert.equal(html, '<p class="err">Its setup step stopped with code 2.</p>');
});

test('what the step printed is escaped', () => {
  const html = helpers().setupFailHtml('Its setup step stopped with code 1.\n<b>no</b>', opts);
  assert.ok(html.includes('&lt;b&gt;no&lt;/b&gt;'));
});

test('a build from what was kept keeps its sentence after the code', () => {
  const f = helpers().setupFail('Its setup step stopped with code 3. Choose your copy of the game to build it from that.\nnothing kept');
  assert.equal(f.head, 'Its setup step stopped with code 3. Choose your copy of the game to build it from that.');
});

test('a failed setup step is not blamed on the copy', () => {
  assert.equal(helpers().setupFailTitle(DEBIAN), 'Setup Didn\'t Finish');
  assert.equal(helpers().setupFailTitle('That file is not a disc image.'), 'That copy didn\'t work');
});

test('the toast for a build in the background has the reason and the last line', () => {
  assert.equal(helpers().setupFailShort('Its setup step stopped with code 1.\nfirst\nlast'), 'Its setup step stopped with code 1. last');
});

test('Copy says which game, version and PC, then the error as the core said it', () => {
  const library = { dkb: { name: 'Donkey Kong 64', tag: 'v1.2.0' } };
  assert.equal(helpers({ library }).setupFailDetails('dkb', DEBIAN), `Donkey Kong 64 · v1.2.0 · Linux · RecompLauncher 0.90.0-beta.68\n${DEBIAN}`);
});

test('Copy puts the details on the clipboard', async () => {
  const calls = [];
  await helpers({ calls }).setupFailAct('copy', 'dkb', DEBIAN);
  assert.equal(calls[0][0], 'clip');
  assert.deepEqual(calls[1], ['toast', 'Copied.']);
});

test('Open Full Log asks the core to open the game\'s setup log', async () => {
  const calls = [];
  await helpers({ calls }).setupFailAct('open', 'dkb', DEBIAN);
  // As JSON: the arguments object was made inside the sandbox, with its own Object.
  assert.equal(JSON.stringify(calls[0]), JSON.stringify(['open_setup_log', { id: 'dkb' }]));
});

test('desktop and Big Picture draw the error with the same helpers', () => {
  assert.match(app, /setupFailHtml\(SU\.err, SU_FAIL\)/);
  assert.match(tv, /setupFailHtml\(TS\.err, \{ p: 'err', row: 'tv-logb', tail: 'tv-tail', button \}\)/);
  assert.match(app, /setupFailTitle\(SU\.err\)/);
  assert.match(tv, /setupFailTitle\(TS\.err\)/);
});

test('the desktop buttons go through Easy Setup\'s own actions', () => {
  const html = helpers().setupFailHtml(DEBIAN, helpers().SU_FAIL);
  assert.deepEqual([...html.matchAll(/data-su="([\w-]+)"/g)].map((m) => m[1]), ['fail-copy', 'fail-open']);
  for (const act of ['fail-copy', 'fail-open']) assert.ok(app.includes(`case '${act}':`), act);
});

test('a failed setup starts on Try Another Copy, not on Copy', () => {
  assert.match(app, /const retry = SU\.stage === 'error' \? \$\('\.su-r \.btn\.main', box\) : null;/);
  assert.match(tv, /const retry = TS\.stage === 'error' \? \$\('\[data-key="ts:0"\]', el\) : null;/);
});

test('the new texts are translated', () => {
  const de = JSON.parse(ui('i18n/de.json'));
  assert.ok(de['Its setup step stopped with code {code}.'].includes('{code}'));
  for (const s of ['Setup Didn\'t Finish', 'Open Full Log']) assert.ok(de[s], s);
});
