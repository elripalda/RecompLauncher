const { test } = require('node:test');
const assert = require('node:assert/strict');
const { stepsContext } = require('./steps-context.cjs');

// Named inputs (setup-inputs.js rows) as the steps draw them, desktop and Big Picture alike.
const row = (id, optional = false, path = null) => ({ id, label: id, role: optional ? 'dlc' : 'disc', optional, path, active: true, file: true });
const named = (inputs) => ({ needs: 'x', kind: '', inputs, prepare_options: [] });

test('four discs and three DLC stay visible, and missing discs block setup', () => {
  const c = stepsContext();
  const su = named([row('disc1', false, '/1.iso'), row('disc2', false, '/2.iso'), row('disc3'), row('disc4'), row('dlc1', true), row('dlc2', true), row('dlc3', true)]);
  const discs = c.stepsView('fixture', su, {}, { name: 'G' });
  assert.equal((discs.match(/class="ss-row(?:"| )/g) || []).length, 4, 'four disc rows on the first step');
  assert.match(discs, /Choose 2 more files to go on\./);
  assert.match(discs, /data-ss="next" disabled/);
  const addons = c.stepsView('fixture', su, { at: 1 }, { name: 'G' });
  assert.equal((addons.match(/class="ss-row(?:"| )/g) || []).length, 3, 'three DLC rows on Add-ons');
  assert.match(addons, /data-ss="run" disabled/, 'Set Up still waits for the discs');
  for (const r of su.inputs.slice(2, 4)) r.path = '/d.iso';
  assert.doesNotMatch(c.stepsView('fixture', su, { at: 1 }, { name: 'G' }), /data-ss="run" disabled/, 'DLC never holds setup back');
});

test('inactive conditional keys are hidden; stored active keys are shown without a path leak', () => {
  const c = stepsContext();
  const su = named([row('base', false, '/game.nsp'), { ...row('keys'), label: 'prod.keys', role: 'key', shared: 'switch.prod-keys', active: false }]);
  assert.doesNotMatch(c.stepsView('fixture', su, {}, { name: 'G' }), /Keys/);
  su.inputs[1].active = true;
  assert.match(c.stepsView('fixture', su, {}, { name: 'G' }), /data-ss="run" disabled/);
  su.inputs[1].path = '/private/shared-keys/switch.prod-keys/prod.keys';
  const html = c.stepsView('fixture', su, {}, { name: 'G' });
  assert.match(html, /Kept once on this PC for every game that needs it/);
  assert.doesNotMatch(html, /\/private\/shared-keys/);
  assert.doesNotMatch(html, /data-ss="run" disabled/);
});

test('profile labels and selected filenames are escaped', () => {
  const c = stepsContext();
  const html = c.stepsView('fixture', named([{ ...row('base', false, '/<img>.iso'), label: '<script>bad</script>' }, row('b2')]), {}, { name: 'G' });
  assert.doesNotMatch(html, /<script>|<img>/);
  assert.match(html, /&lt;script&gt;/);
});

test('optional DLC has its own step, and a chosen one can be removed', () => {
  const c = stepsContext();
  const su = named([row('disc1'), row('dlc1', true, '/bonus.bin')]);
  const m = c.stepsModel('fixture', su, {});
  assert.equal(m.steps.map((s) => s.id).join(), 'game,addons');
  const addons = c.stepsView('fixture', su, { at: 1 }, { name: 'G' });
  assert.match(addons, /<span>Add-ons<\/span><b>1 of 1 added<\/b>/);
  assert.match(addons, /data-ss="clear" data-input="dlc1"[^>]*title="Remove">/);
  assert.match(addons, /data-ss="run" disabled/, 'the disc still holds setup back');
});
