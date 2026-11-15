const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const tf = (text, args) => text.replace(/\{(\w+)\}/g, (_, key) => args[key]);

function context() {
  const calls = [];
  const c = vm.createContext({
    S: {
      os: 'linux', library: {}, settings: { asked_location: true, ui: {} }, jobs: {},
      libs: [{ path: '/games', free: 1000 }], running: new Set(), owned: {}, updates: {}, paused: {},
    },
    byId: (id) => ({ id, name: 'Example' }), parentId: (id) => id, hasGames: () => false,
    turnkey: () => true, autoInstall: () => false, loadLibs: async () => {},
    locationModal: (...args) => calls.push({ command: 'picker', args }),
    document: { body: { classList: { contains: () => false } } }, window: {},
    t: (s) => s, tf, esc: String, fmtB: (size) => size + ' bytes',
    render: () => {}, toast: () => {}, fail: (e) => { throw e; },
    saveSettings: async () => {}, saveUi: async () => {},
    invoke: async (command, args) => {
      calls.push({ command, args: { ...args } });
      if (command === 'install_location') return { target: args.target || (args.dir || '/games') + '/example', free: 1000 };
    },
  });
  vm.runInContext(fn('dlArgs') + fn('installArgs') + fn('startInstall') + fn('resumeDl') + fn('pickBuild'), c);
  return { c, calls };
}

test('new Easy Install always asks for its location even after the first install', async () => {
  const { c, calls } = context();
  await c.startInstall('example');
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, 'picker');
});

test('an older release keeps its tag while asking where to install', async () => {
  const { c, calls } = context();
  await c.startInstall('example', undefined, 'v1');
  assert.equal(calls[0].command, 'picker');
  assert.equal(calls[0].args[1], 'v1');
});

test('ordinary installs still follow Ask Where to Install', async () => {
  const { c, calls } = context();
  c.turnkey = () => false;
  await c.startInstall('example');
  assert.equal(calls.at(-1).command, 'install');
  c.S.settings.ui.ask_where = true;
  await c.startInstall('other');
  assert.equal(calls.at(-1).command, 'picker');
});

test('an exact folder reaches installation without being used as a library parent', async () => {
  const { c, calls } = context();
  await c.startInstall('example', undefined, 'v1', false, '/drive/My récomp 日本');
  const request = calls.find((x) => x.command === 'install').args;
  assert.equal(request.target, '/drive/My récomp 日本');
  assert.equal(request.dir, null);
  assert.equal(request.tag, 'v1');
});

test('a build choice preserves the selected destination and release', async () => {
  const { c, calls } = context();
  await c.startInstall('example', undefined, 'v1', false, '/drive/Chosen');
  c.window.tvPickBuild = (_id, _builds, _current, choose) => { choose('eu.zip'); return true; };
  c.pickBuild('example', [{ name: 'eu.zip' }]);
  const request = calls.at(-1).args;
  assert.equal(request.target, '/drive/Chosen');
  assert.equal(request.tag, 'v1');
  assert.equal(request.file, 'eu.zip');
});

test('choosing another release does not send the previous release asset', async () => {
  const { c, calls } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v1', file: 'old.zip' } };
  await c.startInstall('example', undefined, 'v2', false, '/drive/Chosen');
  assert.equal(calls.at(-1).args.tag, 'v2');
  assert.equal(calls.at(-1).args.file, null);
});

test('an invalid destination stops before starting any download', async () => {
  const { c, calls } = context();
  c.invoke = async (command) => { calls.push({ command }); throw new Error('Folder is occupied'); };
  await assert.rejects(c.startInstall('example', undefined, null, false, '/occupied'), /Folder is occupied/);
  assert.deepEqual(calls.map((x) => x.command), ['install_location']);
});

test('resuming a paused download preserves its destination and selected build', () => {
  const { c, calls } = context();
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v1', file: 'eu.zip' });
  c.resumeDl('example');
  assert.equal(calls[0].args.target, '/drive/Chosen');
  assert.equal(calls[0].args.tag, 'v1');
  assert.equal(calls[0].args.file, 'eu.zip');
});

test('a launcher restart recovers the pending destination and build from backend state', () => {
  const { c, calls } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v1', file: 'eu.zip' } };
  c.resumeDl('example');
  assert.equal(calls[0].args.target, '/drive/Chosen');
  assert.equal(calls[0].args.tag, 'v1');
  assert.equal(calls[0].args.file, 'eu.zip');
});

test('an update skips the picker and retains the installed game folder', async () => {
  const { c, calls } = context();
  c.S.library.example = { path: '/original/game' };
  c.invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === 'install_location') return { target: '/original/game' };
  };
  await c.startInstall('example');
  assert.equal(calls.some((x) => x.command === 'picker'), false);
  assert.equal(calls.at(-1).args.target, '/original/game');
});

test('the selected volume supplies the space warning even for an unregistered exact folder', () => {
  const { c } = context();
  vm.runInContext(fn('pcLacks'), c);
  const warnings = c.pcLacks({ requires: { disk_gb: 1 } }, undefined, { target: '/small/game', free: 100 });
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /100 bytes/);
});

function controllerContext(choices) {
  const { c, calls } = context();
  const starts = [];
  c.document.body.classList.contains = () => true;
  c.window.tvAsk = async () => choices.shift();
  c.startInstall = (...args) => starts.push(args);
  vm.runInContext(fn('locationModal'), c);
  return { c, calls, starts };
}

test('desktop confirmation preserves the exact folder and older release without registering a library', async () => {
  const { c, calls } = context();
  const starts = [], saved = [];
  const pathField = { value: '/drive/My Custom Folder' };
  let modal;
  c.libChoices = () => '';
  c.$ = (selector) => selector === '[data-install-path]' ? pathField : { checked: false };
  c.modal = (_html, click) => { modal = { dataset: {}, addEventListener: () => {}, click }; return modal; };
  c.saveSettings = async (settings) => saved.push(settings);
  c.startInstall = (...args) => starts.push(args);
  vm.runInContext(fn('locationModal'), c);
  await c.locationModal('example', 'v1');
  await modal.click({ dataset: { x: 'yes' } }, () => {});
  assert.equal(starts[0][4], '/drive/My Custom Folder');
  assert.equal(starts[0][2], 'v1');
  assert.deepEqual(JSON.parse(JSON.stringify(saved)), [{ asked_location: true }]);
  assert.equal(calls.some((x) => x.command === 'install'), false);
});

test('controller selection previews a library game folder before starting it', async () => {
  const { c, calls, starts } = controllerContext([1, 0]);
  await c.locationModal('example', 'v1');
  assert.equal(calls[1].args.dir, '/games');
  assert.equal(starts[0][4], '/games/example');
  assert.equal(starts[0][2], 'v1');
});

test('controller browsing selects an exact empty folder', async () => {
  const { c, starts } = controllerContext([2, 0]);
  c.pickFile = async () => '/drive/Exact Folder';
  await c.locationModal('example');
  assert.equal(starts[0][4], '/drive/Exact Folder');
});

test('controller can name a new folder below the browsed parent', async () => {
  const { c, starts } = controllerContext([3, 0]);
  c.pickFile = async () => '/drive/Games';
  c.window.tvType = async () => 'My New Recomp';
  await c.locationModal('example');
  assert.equal(starts[0][4], '/drive/Games/My New Recomp');
});

test('controller creates a Windows folder using a Windows separator', async () => {
  const { c, starts } = controllerContext([3, 0]);
  c.S.os = 'windows';
  c.pickFile = async () => 'E:\\Games';
  c.window.tvType = async () => 'My Recomp';
  await c.locationModal('example');
  assert.equal(starts[0][4], 'E:\\Games\\My Recomp');
});

test('controller cancelling the location prompt starts nothing and saves no setting', async () => {
  const { c, calls, starts } = controllerContext([-1]);
  let saves = 0;
  c.saveSettings = async () => { saves++; };
  await c.locationModal('example');
  assert.equal(starts.length, 0);
  assert.equal(saves, 0);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, 'install_location');
});

test('Big Picture install actions enter the shared location flow', () => {
  assert.doesNotMatch(tv, /startInstall\(id, S\.settings\.library_dir\)/);
});

test('custom managed installs offer folder deletion without registering a library', () => {
  const { c } = context();
  vm.runInContext(fn('ownFolder'), c);
  assert.equal(c.ownFolder({ id: 'example', path: '/drive/My Custom Folder', managed_install: true }), true);
  assert.equal(c.ownFolder({ id: 'example', path: '/drive/My Custom Folder', asset: 'game.zip' }), false);
  assert.equal(c.ownFolder({ id: 'example', path: '/drive/My Custom Folder', local: true, managed_install: true }), false);
});

test('setup displays both the chosen installation and an external build location safely', () => {
  const { c } = context();
  c.S.library.example = { path: '/drive/Chosen' };
  c.esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');
  vm.runInContext(fn('setupLocationHtml'), c);
  const html = c.setupLocationHtml('example', { build_where: '/home/Game <external>' });
  assert.match(html, /Installed in \/drive\/Chosen\./);
  assert.match(html, /\/home\/Game &lt;external>/);
  assert.match(html, /data-act="folder"/);
});
