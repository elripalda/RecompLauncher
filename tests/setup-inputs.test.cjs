const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const code = fs.readFileSync(require('node:path').join(__dirname, '../ui/setup-inputs.js'), 'utf8');
function context() {
  const ctx = vm.createContext({
    t: (x) => x, tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]),
    esc: (x) => String(x ?? '').replace(/[&<>"']/g, (c) => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'}[c])),
    baseName: (x) => x.split(/[\\/]/).pop(), CHECK: '<svg></svg>', DISC_S: '<svg></svg>',
  });
  vm.runInContext(code, ctx); return ctx;
}
const row = (id, optional = false, path = null) => ({ id, label: id, role: optional ? 'dlc' : 'disc', optional, path, active: true, file: true });
test('four discs and three DLC remain visible and missing discs block preparation', () => {
  const c = context(), su = {inputs: [...Array.from({length: 4}, (_, n) => row('Disc ' + (n+1), false, n < 3 ? '/disc.iso' : null)), ...Array.from({length: 3}, (_, n) => row('DLC ' + (n+1), true))]};
  const html = c.namedInputsView('fixture', su);
  assert.equal((html.match(/class="ni-row(?: |")/g) || []).length, 7);
  assert.match(html, /3 of 4 required files selected/);
  assert.match(html, /data-ni="run" disabled/);
  su.inputs[3].path = '/disc4.iso';
  const complete = c.namedInputsView('fixture', su);
  assert.match(complete, /4 of 4 required files selected/);
  assert.doesNotMatch(complete, /data-ni="run" disabled/);
});
test('inactive conditional keys are hidden; stored active keys are shown without a path leak', () => {
  const c = context(), su = {inputs:[row('base', false, '/game.nsp'), {...row('keys'), role:'key', shared:'switch.prod-keys', active:false}]};
  assert.doesNotMatch(c.namedInputsView('fixture', su), /Shared keys/);
  su.inputs[1].active = true;
  assert.match(c.namedInputsView('fixture', su), /data-ni="run" disabled/);
  su.inputs[1].path = '/private/shared-keys/switch.prod-keys/prod.keys';
  const html = c.namedInputsView('fixture', su);
  assert.match(html, /Already configured/);
  assert.doesNotMatch(html, /\/private\/shared-keys/);
  assert.doesNotMatch(html, /data-ni="run" disabled/);
});
test('profile labels and selected filenames are escaped', () => {
  const c = context(), html = c.namedInputsView('fixture', {inputs:[{...row('base', false, '/<img>.iso'), label:'<script>bad</script>'}]});
  assert.doesNotMatch(html, /<script>|<img>/);
  assert.match(html, /&lt;script&gt;/);
});
test('optional DLC starts collapsed and expanding preserves required-file gating', async () => {
  const c = context(), state = {}, su = {inputs:[row('disc1'),row('dlc1',true,'/bonus.bin')]};
  const collapsed = c.namedInputsView('fixture', su, state);
  assert.match(collapsed, /aria-expanded="false"/);
  assert.match(collapsed, /1 of 1 added/);
  assert.match(collapsed, /id="ni-optional-fixture" hidden/);
  await c.namedInputsAction('fixture','toggle-optional',undefined,state);
  const expanded = c.namedInputsView('fixture',su,state);
  assert.match(expanded, /aria-expanded="true"/);
  assert.doesNotMatch(expanded, /id="ni-optional-fixture" hidden/);
  assert.match(expanded, /data-ni="run" disabled/);
  assert.equal(su.inputs[1].path, '/bonus.bin');
});
