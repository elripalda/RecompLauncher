const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const appSrc = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tvSrc = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');

test('status returns settingup when setupJobs has an active setup', () => {
  const c = vm.createContext({
    S: { running: new Set(), settingUp: new Set(), setupJobs: { 'test-game': { step: 'verify', pct: 0.5 } }, jobs: {}, library: {} },
    parentId: (id) => id.split('@')[0],
    forOS: () => true,
  });
  const fnStatus = appSrc.match(/function status\(a\) \{[^]*?\n\}/)[0];
  vm.runInContext(fnStatus, c);
  assert.equal(c.status({ id: 'test-game' }), 'settingup');
  assert.equal(c.status({ id: 'other-game' }), 'get');
});

// The game page's main buttons while a setup runs, with the real status, actions and setupProg.
function actionsWhile(ev) {
  const c = vm.createContext({
    S: { running: new Set(), settingUp: new Set(), setupJobs: { 'test-game': ev }, jobs: {}, library: {} },
    parentId: (id) => id.split('@')[0],
    t: (s) => s,
    esc: String,
    MORE: '',
    XS: '<svg></svg>',
    canPause: () => false,
    forOS: () => true,
  });
  const fnStatus = appSrc.match(/function status\(a\) \{[^]*?\n\}/)[0];
  const fnActions = appSrc.match(/function actions\(a, size = 'big'[^)]*\) \{[^]*?\n\}/)[0];
  const fnProg = appSrc.match(/^function setupProg\([^]*?^}/m)[0];
  vm.runInContext(fnStatus + fnActions + fnProg, c);
  return c.actions({ id: 'test-game' });
}

test('a build running in the background is a bar that reopens its setup, with a cross that cancels it', () => {
  const html = actionsWhile({ step: 'extras', pct: 0.65, extras: true });
  assert.match(html, /class="prog su-prog" data-act="reopen-setup"/);
  assert.match(html, /data-act="setup-cancel"/);
});

test("in its build step, the game page's bar says Building the game and the build step's %", () => {
  const html = actionsWhile({ step: 'extras', pct: 0.65, extras: true });
  assert.match(html, /<div class="track"><i style="width:65%"><\/i><\/div><div class="lbl"><span>Building the game<\/span><span>65%<\/span>/);
});

test("before its build step, the game page's bar only moves: no %", () => {
  const html = actionsWhile({ step: 'verify', pct: 0.65, extras: true });
  assert.match(html, /<div class="track busy"><i style="width:0%"><\/i><\/div><div class="lbl"><span>Building the game<\/span><span><\/span>/);
});

test('a setup with no build step of its own says Setting up', () => {
  const html = actionsWhile({ step: 'verify', pct: 0.4, extras: false });
  assert.match(html, /<span>Setting up<\/span><span><\/span>/);
});

test('the setup window shows a setup running in the background before it shows the steps', () => {
  // Every profile opens the same window now (setupModal): a running job is shown as its progress.
  const fn = appSrc.slice(appSrc.indexOf('async function setupModal(id)'), appSrc.indexOf('\nfunction ssDraw()'));
  const job = fn.indexOf('const activeJob = S.setupJobs && (S.setupJobs[id] || S.setupJobs[parentId(id)]);');
  assert.ok(job > 0);
  assert.ok(fn.indexOf("stage: 'run',", job) > job);
  assert.ok(fn.indexOf("stage: 'steps'") > fn.indexOf("stage: 'run',", job));
});

test('onSetupFinished auto-adds to library and sets setup_ok', async () => {
  let savedUi = null;
  let refreshed = false;
  let toasted = null;
  const c = vm.createContext({
    S: { setup: {}, settings: { ui: {} }, library: {} },
    saved: () => ({}),
    libSend: () => {},
    saveUi: async (patch) => { savedUi = patch; },
    refreshState: async () => { refreshed = true; },
    render: () => {},
    refreshGame: () => {},
    byId: (id) => ({ name: 'Test Recomp' }),
    toast: (msg) => { toasted = msg; },
    tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]),
    invoke: async (cmd, args) => ({ needs: false, copy: '/path/game.iso' }),
  });
  c.window = c;
  const fnOnFinished = appSrc.match(/async function onSetupFinished\(id, watching\) \{[^]*?\n\}/)[0];
  vm.runInContext(fnOnFinished, c);

  await c.onSetupFinished('test-game');
  assert.ok(savedUi);
  assert.ok(savedUi.saved['test-game']);
  assert.equal(savedUi.setup_ok['test-game'], true);
  assert.equal(refreshed, true);
  assert.equal(toasted, 'Test Recomp is ready to play!');
});

// Big Picture's main button and Home's words for it while a setup runs, with the real helpers.
function tvWhile(ev) {
  const c = vm.createContext({
    S: { running: new Set(), settingUp: new Set(), setupJobs: { 'test-game': ev }, jobs: {}, library: {} },
    parentId: (id) => id.split('@')[0],
    t: (s) => s,
    esc: String,
    tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]),
    btn: (key, act, label) => `[btn ${act} ${label}]`,
    forOS: () => true,
  });
  const fnStatus = appSrc.match(/function status\(a\) \{[^]*?\n\}/)[0];
  const fnProg = appSrc.match(/^function setupProg\([^]*?^}/m)[0] + appSrc.match(/^function setupLine\([^]*?^}/m)[0];
  const mainButton = tvSrc.match(/function mainButton\(a, key[^)]*\) \{[^]*?\n  \}/)[0];
  const mainWords = tvSrc.match(/function mainWords\(a\) \{[^]*?\n  \}/)[0];
  vm.runInContext(fnStatus + fnProg + mainButton + mainWords, c);
  return c;
}

test("Big Picture's button says Building the game and the build step's %", () => {
  const c = tvWhile({ step: 'extras', pct: 0.42, extras: true });
  assert.equal(c.mainButton({ id: 'test-game' }, 'k1'), '[btn setup-reopen Building the game · 42%]');
  assert.equal(c.mainWords({ id: 'test-game' }), 'Building the game · 42%');
});

test("before its build step, Big Picture's button has no %", () => {
  const c = tvWhile({ step: 'verify', pct: 0.42, extras: true });
  assert.equal(c.mainWords({ id: 'test-game' }), 'Building the game');
});
