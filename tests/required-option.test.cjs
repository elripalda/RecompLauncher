const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js');
const tv = read('tv.js');

// The setup step's options and the rules for required ones, with what they use from the page.
const start = app.indexOf('function prepOptsHtml(su)');
const src = app.slice(start, app.indexOf('document.addEventListener(\'change\'', start));
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const htmlFill = (s, v) => esc(s).replace(/\{(\w+)\}/g, (m, k) => (k in v ? String(v[k]) : m));
const load = () => new Function('esc', 't', 'htmlFill', `${src} return { prepOptsHtml, prepBlocked, prepGateAttr };`)(esc, (s) => s, htmlFill);
const vs = (on) => ({ id: 'vs', label: 'Build tools', required: true, license: { name: 'VS', url: 'https://example.com/vs' }, on });

test('an unticked required option holds setup back', () => {
  const { prepBlocked } = load();
  assert.equal(prepBlocked({ prepare_options: [vs(false)] }), true);
  assert.equal(prepBlocked({ prepare_options: [vs(true)] }), false);
  // A plain option off holds nothing back.
  assert.equal(prepBlocked({ prepare_options: [{ id: 'hd', label: 'HD', on: false }] }), false);
  assert.equal(prepBlocked('loading'), false);
  assert.equal(prepBlocked(undefined), false);
});

test('a required option is marked, and says why the buttons are off while it is unticked', () => {
  const { prepOptsHtml } = load();
  const off = prepOptsHtml({ prepare_options: [vs(false)] });
  assert.match(off, /<b>Build tools<\/b> <span class="su-req-tag">Required<\/span>/);
  assert.match(off, /data-popt="vs" aria-required="true"><span class="switch"/);
  assert.match(off, /<p class="su-req" role="status">Turn on the option marked Required to continue<\/p>/);
  assert.match(prepOptsHtml({ prepare_options: [vs(true)] }), /<p class="su-req" role="status" hidden>/);
});

test('the buttons that start setup are off until the required ticks are on', () => {
  const { prepGateAttr } = load();
  assert.equal(prepGateAttr({ prepare_options: [vs(false)] }, false), ' data-req-gate="" disabled');
  assert.equal(prepGateAttr({ prepare_options: [vs(true)] }, false), ' data-req-gate=""');
  // Off for another reason too (files still to choose): it stays off after the tick.
  assert.equal(prepGateAttr({ prepare_options: [vs(true)] }, true), ' data-req-gate="off" disabled');
});

test('desktop: Set Up waits for the required ticks (choosing files starts nothing in the steps)', () => {
  assert.match(app, /if \(prepBlocked\(S\.setup\[SU\.id\]\)\) \{ SU\.stage = 'steps'; return suDraw\(\); \}\n\s+await SU\.optsSaving;/);
  assert.match(app, /prepGate\(SU\.el, su\);/);
  // The steps' own rule (setup-steps.js), tested in setup-steps.test.cjs.
  assert.match(read('setup-steps.js'), /const reqOff = !skipBuild && prepBlocked\(su\);/);
  assert.match(read('setup-inputs.js'), /data-ni="run"\$\{typeof prepGateAttr === 'function' \? prepGateAttr\(su, !complete \|\| ns\.busy\)/);
});

test('Big Picture: the items that start setup are dimmed, and the inputs window ticks are reachable', () => {
  assert.match(tv, /const gated = \(label, sub, run, cls = ''\) => \(blocked \? it\(label, sub, \(\) => toast\(prepReq\(\)\), `\$\{cls\} off`\) : it\(label, sub, run, cls\)\);/);
  assert.match(tv, /if \(prepBlocked\(S\.setup\[TS\.id\]\)\) \{ toast\(prepReq\(\)\); TS\.stage = 'pick'; return tsDraw\(\); \}/);
  assert.match(tv, /row\.dataset\.f = ''; row\.dataset\.key = 'po:' \+ o\.id;/);
});
