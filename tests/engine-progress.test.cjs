const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../ui/app.js'), 'utf8');
function render(p) {
  const ctx = vm.createContext({p, t:s=>s, tf:(s,a)=>s.replace(/\{(\w+)\}/g,(_,k)=>a[k]), fmtNum:n=>String(n), fmtB:n=>`${n} B`, esc:s=>String(s).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('"','&quot;')});
  vm.runInContext(source.match(/^function engineSetupProgress\([^]*?^}/m)?.[0] || source.match(/^function engineSetupProgress.*$/m)[0],ctx);
  return vm.runInContext('engineSetupProgress(p)',ctx);
}
test('engine progress uses work fraction and labels ETA as current-step time', () => {
  const html=render({text:'Mac shaders · compiling',fraction:0.525,stage:5,stages:11,done:951,total:1812,unit:'shaders',eta_seconds:5.46,elapsed_seconds:90});
  assert.match(html,/52%/);
  assert.match(html,/value="52"/);
  assert.match(html,/Stage 5 of 11/);
  assert.match(html,/951 of 1812 shaders/);
  assert.match(html,/About 6 s remaining for this step/);
  assert.match(html,/Stage elapsed: 1 min 30 s/);
});
test('unknown progress stays indeterminate and does not invent an ETA', () => {
  const html=render({text:'Checking <inputs>',fraction:null,eta_seconds:null,elapsed_seconds:null});
  assert.match(html,/<progress[^>]*>/);
  assert.doesNotMatch(html,/value=|remaining|%/);
  assert.match(html,/Checking &lt;inputs>/);
});
test('zero progress is measurable but an exhausted ETA never says the install is complete', () => {
  const html=render({text:'Compiling',fraction:0,eta_seconds:0,done:0,total:100,unit:'shaders',estimated:true});
  assert.match(html,/value="0"/);
  assert.match(html,/0 of ~100 shaders/);
  assert.doesNotMatch(html,/remaining|complete|100%/);
});
