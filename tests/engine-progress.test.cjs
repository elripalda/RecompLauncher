const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../ui/app.js'), 'utf8');
const tvSource = fs.readFileSync(require('node:path').join(__dirname, '../ui/tv.js'), 'utf8');
function render(p) {
  const ctx = vm.createContext({p, t:s=>s, tf:(s,a)=>s.replace(/\{(\w+)\}/g,(_,k)=>a[k]), fmtNum:n=>String(n), fmtB:n=>`${n} B`, esc:s=>String(s).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('"','&quot;')});
  for (const name of ['engineSetupPercent', 'engineSetupProgress']) {
    vm.runInContext(source.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0], ctx);
  }
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

test('Big Picture reports indeterminate background engine work without a made-up percentage', () => {
  const progress = { step: 'extras', pct: 0, engine_progress: { text: 'Compiling shaders', fraction: null } };
  const ctx = vm.createContext({
    S: { setupJobs: { g: progress }, library: { g: { engine_build: { state: 'pending' } } } },
    status: () => 'settingup', btn: (_key, _act, label) => label, t: String, esc: String,
    tf: (s, fields) => s.replace('{pct}', fields.pct),
  });
  for (const name of ['engineSetupPercent', 'setupProgressLabel']) {
    vm.runInContext(source.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0], ctx);
  }
  vm.runInContext(tvSource.match(/^  function mainButton\([^]*?^  }/m)[0], ctx);
  assert.match(ctx.mainButton({ id: 'g' }, 'play'), /Compiling shaders/);
  assert.doesNotMatch(ctx.mainButton({ id: 'g' }, 'play'), /%/);
  progress.engine_progress.fraction = 0.525;
  assert.match(ctx.mainButton({ id: 'g' }, 'play'), /52%/);
});

test('Big Picture keeps the background and cancel controls in place as engine progress changes', () => {
  const panel = { dataset: {} };
  const progress = { outerHTML: '' };
  const event = { engine_progress: { text: 'Compiling shaders', fraction: 0.25 } };
  const ctx = vm.createContext({
    S: { setup: {}, library: { g: { engine_build: { state: 'pending' } } } },
    TV: {}, TS: { id: 'g', stage: 'run', step: 'extras', steps: { extras: event } },
    stage: () => ({}), byId: () => ({ name: 'Game' }), focused: () => null,
    $: (s) => s === '.tv-setup:not(.out)' ? panel : s === '.engine-progress' ? progress : null,
    esc: String, t: String, gameMediaHtml: () => '', engineSetupProgress: render,
  });
  vm.runInContext(tvSource.match(/^  function tsDraw\([^]*?^  }/m)[0], ctx);
  ctx.tsDraw();
  const first = panel.innerHTML;
  assert.match(first, /Run in Background/);
  assert.match(first, /Cancel Setup/);
  assert.match(first, /value="25"/);
  event.engine_progress.fraction = 0.5;
  ctx.tsDraw();
  assert.equal(panel.innerHTML, first);
  assert.match(progress.outerHTML, /value="50"/);
});
