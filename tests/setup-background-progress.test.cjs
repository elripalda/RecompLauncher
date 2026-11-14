const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const appSrc = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tvSrc = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const inputsSrc = fs.readFileSync(path.join(__dirname, '../ui/setup-inputs.js'), 'utf8');

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

test('actions renders clickable reopen-setup button with stage and percentage and direct cancel button', () => {
  const c = vm.createContext({
    S: { running: new Set(), settingUp: new Set(), setupJobs: { 'test-game': { step: 'verify', pct: 0.65 } }, jobs: {}, library: {} },
    parentId: (id) => id.split('@')[0],
    t: (s) => s,
    esc: String,
    MORE: '',
    XS: '<svg></svg>',
    canPause: () => false,
    forOS: () => true,
  });
  const fnStatus = appSrc.match(/function status\(a\) \{[^]*?\n\}/)[0];
  const fnActions = appSrc.match(/function actions\(a, size = 'big'\) \{[^]*?\n\}/)[0];
  vm.runInContext(fnStatus, c);
  vm.runInContext(fnActions, c);

  const html = c.actions({ id: 'test-game' });
  assert.match(html, /data-act="reopen-setup"/);
  assert.match(html, /data-act="setup-cancel"/);
  assert.match(html, /class="prog su-prog"/);
  assert.match(html, /width:65%/);
  assert.match(html, /65%/);
  assert.match(html, /Checking/);
});

test('namedSetup delegates to setupModal when job is active in background', () => {
  let reopened = null;
  const c = vm.createContext({
    S: { setupJobs: { 'longshot': { step: 'extras', pct: 0.8 } } },
    parentId: (id) => id.split('@')[0],
    setupModal: (id) => { reopened = id; },
    closeSetup: () => {},
  });
  vm.runInContext(inputsSrc, c);
  c.namedSetup('longshot');
  assert.equal(reopened, 'longshot');
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
  const fnOnFinished = appSrc.match(/async function onSetupFinished\(id\) \{[^]*?\n\}/)[0];
  vm.runInContext(fnOnFinished, c);

  await c.onSetupFinished('test-game');
  assert.ok(savedUi);
  assert.ok(savedUi.saved['test-game']);
  assert.equal(savedUi.setup_ok['test-game'], true);
  assert.equal(refreshed, true);
  assert.equal(toasted, 'Test Recomp is ready to play!');
});

test('TV mode mainButton and mainWords display settingup progress', () => {
  const c = vm.createContext({
    S: { running: new Set(), settingUp: new Set(), setupJobs: { 'test-game': { step: 'verify', pct: 0.42 } }, jobs: {}, library: {} },
    parentId: (id) => id.split('@')[0],
    t: (s) => s,
    esc: String,
    tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]),
    btn: (key, act, label) => `[btn ${act} ${label}]`,
    forOS: () => true,
  });
  const fnStatus = appSrc.match(/function status\(a\) \{[^]*?\n\}/)[0];
  vm.runInContext(fnStatus, c);

  const mainBtnMatch = tvSrc.match(/function mainButton\(a, key\) \{[^]*?\n  \}/)[0];
  vm.runInContext(mainBtnMatch, c);
  const btnHtml = c.mainButton({ id: 'test-game' }, 'k1');
  assert.match(btnHtml, /setup-reopen/);
  assert.match(btnHtml, /Setting up 42%/);

  const mainWordsMatch = tvSrc.match(/function mainWords\(a\) \{[^]*?\n  \}/)[0];
  vm.runInContext(mainWordsMatch, c);
  const words = c.mainWords({ id: 'test-game' });
  assert.equal(words, 'Setting up 42%');
});
