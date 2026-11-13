const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { stepsContext } = require('./steps-context.cjs');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

test('release refresh cannot schedule a prepared engine game even with stale auto-update settings', async () => {
  const started = [];
  const ctx = vm.createContext({
    S: {
      library: {
        prepared: { name: 'Prepared', engine_build: { format: 99 }, auto_update: true },
        malformed: { name: 'Malformed', engine_build: null, auto_update: true },
        legacy: { name: 'Legacy', auto_update: true },
      },
      running: new Set(), jobs: {}, view: 'library', settings: { ui: {}, auto_update_all: true },
    },
    invoke: async () => ({ prepared: 'v2', malformed: 'v2', legacy: 'v2' }),
    window: {}, document: { body: { classList: { contains: () => false } } },
    renderSide() {}, render() {}, toast() {}, updatingLine: () => '', t: (s) => s, tf: (s) => s,
    byId: () => ({}), badVersion: () => false,
    startInstall: (id) => started.push(id), fail: (error) => { throw error; },
  });
  vm.runInContext('let updateCheckRevision = 0;', ctx);
  for (const name of ['autoUpdateAll', 'autoUpSkipped']) {
    vm.runInContext(source.match(new RegExp(`^const ${name} = .*;$`, 'm'))[0], ctx);
  }
  for (const name of ['heldIds', 'holdUpdates', 'autoUpdates']) {
    const start = source.indexOf(`function ${name}(`);
    const end = source.indexOf(name === 'heldIds' ? '\n' : '\n}', start);
    vm.runInContext(source.slice(start, name === 'heldIds' ? end : end + 2), ctx);
  }
  vm.runInContext(source.match(/^async function checkUpdates\([^]*?^}/m)[0], ctx);
  assert.equal(await vm.runInContext('checkUpdates(false)', ctx), true);
  assert.deepEqual(started, ['legacy']);
  assert.deepEqual(Object.keys(ctx.S.updates), ['legacy']);
});

test('prepared games do not gain setup steps from a catalog refresh or stale installer state', () => {
  const ctx = vm.createContext({ S: { library: {
    prepared: { engine_build: { format: 1 }, installer_setup: 'old-setup' },
    malformed: { engine_build: null }, legacy: {},
  } }, setupDone: () => false });
  for (const name of ['needsSetup', 'installerDue', 'setupDue']) {
    vm.runInContext(source.match(new RegExp(`^const ${name} = .*;$`, 'm'))[0], ctx);
  }
  assert.equal(vm.runInContext("setupDue({ id: 'prepared', setup_required: true })", ctx), false);
  assert.equal(vm.runInContext("setupDue({ id: 'malformed', setup_required: true })", ctx), false);
  assert.equal(vm.runInContext("setupDue({ id: 'legacy', setup_required: true })", ctx), true);
});

test('engine Easy Install registers the game before asking for its files', async () => {
  const calls = [];
  const ctx = vm.createContext({
    S: {library:{}, settings:{asked_location:true}, running:new Set(), jobs:{}, owned:{}, updates:{}},
    parentId: (id) => id, byId: (id) => ({id,engine:'nxnative'}), turnkey: () => true,
    subOf: () => false, gamesOf: () => [],
    document: {body:{classList:{contains:()=>false}}},
    t: (s)=>s, esc: (s)=>s, ask: () => {throw new Error('No extra prompt expected');}, render() {}, dlArgs() {},
    pickWhileDownloading: () => { throw new Error('The shared Setup Steps ask for the registered inputs'); },
    invoke: async (command,args) => calls.push({command,args}), fail: (e)=>{throw e;},
  });
  vm.runInContext(source.match(/^const autoInstall = .*;$/m)[0], ctx);
  vm.runInContext(source.match(/^async function startInstall\([^]*?^}/m)[0],ctx);
  await vm.runInContext("startInstall('nxnative-test')",ctx);
  assert.equal(calls.length,1);
  assert.equal(calls[0].command,'install');
  assert.equal(calls[0].args.id,'nxnative-test');
});

test('engine updates explain the pinned build without offering a reinstall', () => {
  const ctx = vm.createContext({S:{library:{g:{engine_build:{package:{engine:'nxnative',packaging_revision:'old-working-pin'}}}}}, t:s=>s, esc:s=>s});
  vm.runInContext(source.match(/^function engineVersionCard\([^]*?^}/m)[0],ctx);
  const html = vm.runInContext("engineVersionCard({id:'g',engine:'nxnative',engine_version:'new recommendation'})",ctx);
  assert.ok(html.includes('old-working-pin'));
  assert.ok(!html.includes('new recommendation'));
  assert.ok(!html.includes('data-act="install"'));
});

test('Versions keeps an unreadable package pin visible without displaying the current recommendation', () => {
  const S = { library: { g: { engine_build: { package: { upstream_revision: 42 } } } } };
  const ctx = vm.createContext({ S, t: String, esc: String });
  vm.runInContext(source.match(/^function engineVersionCard\([^]*?^}/m)[0], ctx);
  const html = ctx.engineVersionCard({ id: 'g', engine_version: 'current recommendation' });
  assert.match(html, /Not known/);
  assert.doesNotMatch(html, /current recommendation/);
});

test('prepared engine games leave the needs-your-copy filter while pending ones remain', () => {
  const ctx=vm.createContext({S:{setup:{},easy:new Set(['pending','prepared'])}});
  vm.runInContext(source.match(/^function needsCopySetup\([^]*?^}/m)[0],ctx);
  assert.equal(vm.runInContext("needsCopySetup({id:'prepared',engine_build:{format:1,game:{},package:{}}})",ctx),false);
  assert.equal(vm.runInContext("needsCopySetup({id:'pending',engine_build:{format:1,state:'pending'}})",ctx),true);
});

test('game version comes from its game pin, not its frontend package tag', () => {
  const ctx=vm.createContext({t:s=>s});
  vm.runInContext(source.match(/^function installedVersion\([^]*?^}/m)[0],ctx);
  assert.equal(vm.runInContext("installedVersion({tag:'NX2Apple v1.4.0',engine_build:{game:{version:'1.5.12620'}}})",ctx),'1.5.12620');
  assert.equal(vm.runInContext("installedVersion({tag:'v2.0'})",ctx),'v2.0');
});

test('Manage displays a prepared game version without ordinary release controls', () => {
  const ctx = vm.createContext({
    t: String, esc: String, isLab: () => false, isSF: () => false,
    heldIds: () => new Set(), autoUpdateAll: () => true, autoUpdates: () => true,
  });
  for (const name of ['installedVersion', 'manageUpdateRows']) {
    vm.runInContext(source.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0], ctx);
  }
  const rows = ctx.manageUpdateRows('g', {
    repo: 'developer/engine',
    tag: 'engine package release',
    engine_build: { game: { version: '2.2.0' } },
  }).join('');
  assert.match(rows, /Installed version.*2\.2\.0/);
  assert.doesNotMatch(rows, /data-act="(?:auto-up|hold-up|build)"|engine package release/);
});

test('Manage keeps custom launch options available only for ordinary games', () => {
  const ctx = vm.createContext({
    S: { setup: {}, easy: new Set() }, t: String, esc: String,
    easyRow: () => '', programRow: () => '', byHand: () => false,
  });
  vm.runInContext(source.match(/^function manageStartRows\([^]*?^}/m)[0], ctx);
  const args = 'saved launch option';
  const legacyRows = ctx.manageStartRows('g', { args }).join('');
  assert.match(legacyRows, /data-act="launch-opts"/);
  assert.match(legacyRows, /saved launch option/);
  const engineRows = ctx.manageStartRows('g', { args, engine_build: { format: 1 } }).join('');
  assert.doesNotMatch(engineRows, /data-act="launch-opts"|saved launch option/);
});

test('an engine record also blocks the launch-options dialog when invoked directly', () => {
  const ctx = vm.createContext({
    S: { library: { g: { name: 'Game', engine_build: null } } },
    t: String, tf: String, esc: String,
    modal: () => assert.fail('An engine record must not open the launch-options dialog'),
  });
  vm.runInContext(source.match(/^function launchOptions\([^]*?^}/m)[0], ctx);
  ctx.launchOptions('g');
});

test('shared Setup Steps keep the engine file picker without offering to skip preparation', () => {
  const S = { settings: { ui: {} }, owned: {}, easyAll: new Set(['g']), library: {
    g: { engine_build: { state: 'pending' } },
  } };
  const ctx = stepsContext({}, { S });
  const su = { needs: 'Your game', inputs: [
    { id: 'game', label: 'Base game', role: 'game', file: true, active: true },
  ] };
  const view = () => ctx.stepsView('g', su, {}, { name: 'Game', engine: 'nxnative' });
  assert.match(view(), /Base game/);
  assert.doesNotMatch(view(), /data-ss="skip-easy"/);
});
