const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const test = require('node:test');
const { runInNewContext } = require('node:vm');

// A setup step that stopped: the player sees the launcher's line (translated, with the step's
// code) and the last few lines the step printed in a box, never one cut-off line or the whole log.
// Desktop and Big Picture draw it with the same helpers.
const ui = (f) => readFileSync(join(__dirname, '../ui', f), 'utf8');
const app = ui('app.js');
const tv = ui('tv.js');
const between = (src, from, to) => src.slice(src.indexOf(from), src.indexOf(to));
const helpersSrc = between(app, 'const SETUP_TAIL_LINES', '// Steps that finish quickly still show');

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const tf = (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]);
const helpers = runInNewContext(`${helpersSrc}\n({ setupFail, setupFailHtml, setupFailTitle, setupFailShort, SETUP_TAIL_LINES })`, { t: String, tf, esc });

const DEBIAN = [
  'Its setup step stopped with code 1.',
  'Checking what this PC has...',
  'Missing: Vulkan SDK, .NET 10 SDK',
  'On Debian or Ubuntu, install them with:',
  '  sudo apt install cmake ninja-build',
  '  libvulkan-dev vulkan-validation-layers spirv-tools dotnet-sdk-10.0 git',
].join('\n');

test('the launcher line of a failed setup step is read with its code', () => {
  const f = helpers.setupFail(DEBIAN);
  assert.equal(f.head, 'Its setup step stopped with code 1.');
  assert.equal(f.step, true);
});

test('the lines the step printed are kept apart from the launcher line', () => {
  assert.equal(helpers.setupFail(DEBIAN).printed.length, 5);
});

test('the error screen shows only the last few printed lines', () => {
  const html = helpers.setupFailHtml(DEBIAN, 'su-err', 'su-tail');
  const box = html.match(/<pre class="su-tail">([\s\S]*)<\/pre>/)[1];
  assert.equal(box.split('\n').length, helpers.SETUP_TAIL_LINES);
  assert.ok(!box.includes('Checking what this PC has'));
  assert.ok(box.endsWith('dotnet-sdk-10.0 git'));
});

test('a step that printed nothing shows no box', () => {
  const html = helpers.setupFailHtml('Its setup step stopped with code 2.', 'su-err', 'su-tail');
  assert.equal(html, '<p class="su-err">Its setup step stopped with code 2.</p>');
});

test('what the step printed is escaped', () => {
  const html = helpers.setupFailHtml('Its setup step stopped with code 1.\n<b>no</b>', 'su-err', 'su-tail');
  assert.ok(html.includes('&lt;b&gt;no&lt;/b&gt;'));
});

test('a build from what was kept keeps its sentence after the code', () => {
  const f = helpers.setupFail('Its setup step stopped with code 3. Choose your copy of the game to build it from that.\nnothing kept');
  assert.equal(f.head, 'Its setup step stopped with code 3. Choose your copy of the game to build it from that.');
});

test('a failed setup step is not blamed on the copy', () => {
  assert.equal(helpers.setupFailTitle(DEBIAN), 'Setup Didn\'t Finish');
  assert.equal(helpers.setupFailTitle('That file is not a disc image.'), 'That copy didn\'t work');
});

test('the toast for a build in the background has the reason and the last line', () => {
  assert.equal(helpers.setupFailShort('Its setup step stopped with code 1.\nfirst\nlast'), 'Its setup step stopped with code 1. last');
});

test('desktop and Big Picture draw the error with the same helpers', () => {
  assert.match(app, /setupFailHtml\(SU\.err, 'su-err', 'su-tail'\)/);
  assert.match(tv, /setupFailHtml\(TS\.err, 'err', 'tv-tail'\)/);
  assert.match(app, /setupFailTitle\(SU\.err\)/);
  assert.match(tv, /setupFailTitle\(TS\.err\)/);
});

test('the new texts are translated', () => {
  const de = JSON.parse(ui('i18n/de.json'));
  assert.ok(de['Its setup step stopped with code {code}.'].includes('{code}'));
  assert.ok(de['Setup Didn\'t Finish']);
});
