const { test } = require('node:test');
const assert = require('node:assert/strict');
const { stepsContext, read } = require('./steps-context.cjs');
const app = read('app.js');
const tv = read('tv.js');

// The rule for required options (app.js), with what it uses from the page.
const start = app.indexOf('const prepReq = ');
const src = app.slice(start, app.indexOf('// The ways to give the game\'s copy', start));
const load = () => new Function('t', `${src} return { prepBlocked, prepReq };`)((s) => s);
const vs = (on) => ({ id: 'vs', label: 'Build tools', required: true, license: { name: 'VS', url: 'https://example.com/vs' }, on });
const su = (opts) => ({ needs: 'x', kind: 'iso', copy: '/g.iso', inputs: [], prepare_options: opts });

test('an unticked required option holds setup back', () => {
  const { prepBlocked } = load();
  assert.equal(prepBlocked({ prepare_options: [vs(false)] }), true);
  assert.equal(prepBlocked({ prepare_options: [vs(true)] }), false);
  // A plain option off holds nothing back.
  assert.equal(prepBlocked({ prepare_options: [{ id: 'hd', label: 'HD', on: false }] }), false);
  assert.equal(prepBlocked('loading'), false);
  assert.equal(prepBlocked(undefined), false);
});

test('a required option is marked, and the line by Set Up says why it waits', () => {
  const c = stepsContext();
  const off = c.stepsView('g', su([vs(false)]), { at: 1 }, { name: 'G' });
  assert.match(off, /<b>Build tools<\/b> <span class="su-req-tag">Required<\/span>/);
  assert.match(off, /data-popt="vs" aria-required="true"><span class="switch"/);
  assert.match(off, /<p class="why" role="status">Turn on the option marked Required to continue<\/p>/);
  assert.match(c.stepsView('g', su([vs(true)]), { at: 1 }, { name: 'G' }), /<p class="why" role="status"><svg class="ok"><\/svg>Ready to set up\.<\/p>/);
});

test('only Set Up waits for a required option: choosing files and Next start nothing', () => {
  const c = stepsContext();
  const s = su([vs(false)]);
  assert.doesNotMatch(c.stepsView('g', s, {}, { name: 'G' }), /data-ss="(file|folder|next)"[^>]* disabled/);
  assert.match(c.stepsView('g', s, { at: 1 }, { name: 'G' }), /data-ss="run" disabled/);
});

test('desktop: Set Up waits for the required ticks, and a change draws the steps again', () => {
  assert.match(app, /if \(prepBlocked\(S\.setup\[SU\.id\]\)\) \{ SU\.stage = 'steps'; return suDraw\(\); \}\n\s+await SU\.optsSaving;/);
  assert.match(app, /if \(SU\.stage === 'steps'\) ssDraw\(\);/);
  assert.match(read('setup-steps.js'), /const reqOff = !skipBuild && prepBlocked\(su\);/);
});

test('Big Picture: the same rule, and the option rows are items the controller reaches', () => {
  assert.match(tv, /if \(prepBlocked\(S\.setup\[TS\.id\]\)\) \{ toast\(prepReq\(\)\); TS\.stage = 'steps'; return tsDraw\(\); \}/);
  assert.match(tv, /row\.dataset\.f = ''; row\.dataset\.key = 'po:' \+ o\.id;/);
  assert.match(tv, /\$\$\('\.ss-opts a\[data-url\]', el\)/, 'a license can be opened from the controller');
});
