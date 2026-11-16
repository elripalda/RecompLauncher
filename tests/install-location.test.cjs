const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const named = fs.readFileSync(path.join(__dirname, '../ui/setup-inputs.js'), 'utf8');
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
  vm.runInContext(fn('dlArgs') + fn('installArgs') + fn('startInstall') + fn('resumeDl') + fn('pickBuild') + fn('pendingInstallItems') + fn('cancelInstall'), c);
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

test('explicit newest release replaces the failed older release and asset', async () => {
  const { c, calls } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v1', file: 'old.zip' } };
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v1', file: 'old.zip' });
  await c.startInstall('example', undefined, null, false, '/drive/Chosen');
  assert.equal(calls.at(-1).args.tag, null);
  assert.equal(calls.at(-1).args.file, null);
  assert.equal(calls.at(-1).args.resetRelease, true);
});

test('Big Picture newest choice survives location confirmation and clears the old release', async () => {
  const { c, calls } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v1', file: 'old.zip' } };
  let items;
  c.sheet = (_title, choices) => { items = choices; };
  c.date = () => '';
  c.invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === 'versions') return [{ tag: 'v2', newest: true }];
    if (command === 'install_location') return { target: '/drive/Chosen' };
  };
  vm.runInContext(tv.match(/^  async function versionsSheet\([^]*?^  }/m)[0], c);
  await c.versionsSheet('example');
  await items[0].run();
  const picker = calls.at(-1);
  assert.equal(picker.command, 'picker');
  assert.equal(picker.args[1], null);
  c.document.body.classList.contains = () => true;
  c.window.tvAsk = async () => 0;
  vm.runInContext(fn('locationModal'), c);
  await c.locationModal('example', picker.args[1]);
  assert.equal(calls.at(-1).command, 'install');
  assert.equal(calls.at(-1).args.tag, null);
  assert.equal(calls.at(-1).args.file, null);
  assert.equal(calls.at(-1).args.resetRelease, true);
});

test('build selection and resume consume a release reset only on the initial request', async () => {
  const { c, calls } = context();
  await c.startInstall('example', undefined, null, false, '/drive/Chosen');
  assert.equal(calls.at(-1).args.resetRelease, true);
  c.window.tvPickBuild = (_id, _builds, _current, choose) => { choose('eu.zip'); return true; };
  c.pickBuild('example', [{ name: 'eu.zip' }]);
  assert.equal(calls.at(-1).args.resetRelease, false);
  c.resumeDl('example');
  assert.equal(calls.at(-1).args.resetRelease, false);
  assert.equal(calls.at(-1).args.file, 'eu.zip');
});

test('an invalid destination stops before starting any download', async () => {
  const { c, calls } = context();
  c.invoke = async (command) => { calls.push({ command }); throw new Error('Folder is occupied'); };
  await assert.rejects(c.startInstall('example', undefined, null, false, '/occupied'), /Folder is occupied/);
  assert.deepEqual(calls.map((x) => x.command), ['install_location']);
});

test('a refused destination change keeps the original resumable request', async () => {
  const { c } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Original', tag: 'v1', file: 'game.zip' } };
  c.invoke = async (command, args) => {
    if (command === 'install_location') return { target: args.target };
    if (command === 'install') throw new Error('Retry the partial extraction in its original folder');
  };
  await assert.rejects(c.startInstall('example', undefined, undefined, false, '/drive/Other'), /partial extraction/);
  assert.equal(c.installArgs('example').target, '/drive/Original');
  assert.equal(c.installArgs('example').tag, 'v1');
  assert.equal(c.installArgs('example').file, 'game.zip');
});

test('resuming a paused download preserves its destination and selected build', () => {
  const { c, calls } = context();
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v1', file: 'eu.zip' });
  c.resumeDl('example');
  assert.equal(calls[0].args.target, '/drive/Chosen');
  assert.equal(calls[0].args.tag, 'v1');
  assert.equal(calls[0].args.file, 'eu.zip');
});

test('desktop paused cancellation clears the old release before a fresh install without an event', async () => {
  const { c, calls } = context();
  c.S.paused.example = { phase: 'paused' };
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v1', file: 'old.zip' } };
  c.S.prePicked = { example: '/source/old.iso' };
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v1', file: 'old.zip' });
  vm.runInContext(fn('act'), c);
  await c.act({ dataset: { act: 'cancel', id: 'example' } });
  await c.startInstall('example', undefined, undefined, false, '/drive/New');
  assert.equal(calls[0].command, 'cancel');
  assert.equal(calls.at(-1).args.tag, null);
  assert.equal(calls.at(-1).args.file, null);
  assert.equal(calls.at(-1).args.source, null);
  assert.equal(c.S.paused.example, undefined);
  assert.equal(c.S.pendingInstalls.example, undefined);
});

test('Big Picture paused cancellation also clears the old release without an event', async () => {
  const { c, calls } = context();
  c.S.paused.example = { phase: 'paused' };
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v1', file: 'old.zip' } };
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v1', file: 'old.zip' });
  c.TV = { sheet: null };
  c.draw = () => {};
  vm.runInContext(tv.match(/^  async function activate\([^]*?^  }/m)[0], c);
  await c.activate({ dataset: { tvact: 'cancel', id: 'example' } });
  await c.startInstall('example', undefined, undefined, false, '/drive/New');
  assert.equal(calls[0].command, 'cancel');
  assert.equal(calls.at(-1).args.tag, null);
  assert.equal(calls.at(-1).args.file, null);
  assert.equal(c.S.paused.example, undefined);
});

test('a failed cancellation retains the request for resuming', async () => {
  const { c } = context();
  c.S.paused.example = { phase: 'paused' };
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v1', file: 'old.zip' });
  c.invoke = async () => { throw new Error('Cancellation failed'); };
  await assert.rejects(c.cancelInstall('example'), /Cancellation failed/);
  assert.equal(c.installArgs('example').tag, 'v1');
  assert.equal(c.S.paused.example.phase, 'paused');
});

test('skipping a failed release after restarting lets the next ordinary update choose its release', async () => {
  const { c, calls } = context();
  c.S.library.example = { path: '/drive/Chosen', tag: 'v1' };
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', selected_tag: 'v2', file: 'v2.zip' } };
  c.S.updates.example = 'v2';
  c.S.recent = [{ id: 'example', phase: 'error' }];
  c.renderSide = () => {};
  c.badVersion = () => false;
  c.invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === 'install_location') return { target: c.S.library[args.id].path };
  };
  c.saveUi = async (settings) => {
    calls.push({ command: 'save-ui', settings });
    Object.assign(c.S.settings.ui, settings);
  };
  vm.runInContext(fn('act'), c);
  await c.act({ dataset: { act: 'skip-ver', id: 'example' } });
  assert.equal(calls[0].command, 'cancel');
  assert.equal(calls[1].command, 'save-ui');
  assert.equal(c.S.settings.ui.skip_ver.example, 'v2');
  assert.equal(c.S.pendingInstalls.example, undefined);
  assert.equal(c.S.updates.example, undefined);
  assert.equal(c.S.library.example.path, '/drive/Chosen');
  c.S.updates.example = 'v3';
  await c.startInstall('example');
  assert.equal(calls.at(-1).args.tag, null);
  assert.equal(calls.at(-1).args.file, null);
  assert.equal(calls.at(-1).args.target, '/drive/Chosen');
});

test('skipping a failed release also clears a request cached in the current session', async () => {
  const { c } = context();
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v2', file: 'v2.zip' });
  c.S.updates.example = 'v2';
  c.S.recent = [];
  c.renderSide = () => {};
  vm.runInContext(fn('act'), c);
  await c.act({ dataset: { act: 'skip-ver', id: 'example' } });
  assert.equal(c.installArgs('example').tag, null);
  assert.equal(c.installArgs('example').file, null);
});

test('failed cancellation prevents skipping a release and preserves its retry state', async () => {
  const { c } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', tag: 'v2', file: 'v2.zip' } };
  c.S.updates.example = 'v2';
  c.invoke = async () => { throw new Error('Cancellation failed'); };
  vm.runInContext(fn('act'), c);
  await assert.rejects(c.act({ dataset: { act: 'skip-ver', id: 'example' } }), /Cancellation failed/);
  assert.equal(c.S.updates.example, 'v2');
  assert.equal(c.S.pendingInstalls.example.file, 'v2.zip');
  assert.equal(c.S.settings.ui.skip_ver, undefined);
});

test('a persisted failed update can be cancelled from the desktop game menu after restart', async () => {
  const { c, calls } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen', selected_tag: 'v2', file: 'v2.zip' } };
  vm.runInContext(fn('act'), c);
  const [item] = c.pendingInstallItems('example');
  assert.equal(item.label, 'Cancel the download');
  assert.match(fn('gameMenu'), /\.\.\.pendingInstallItems\(id\)/);
  await c.act({ dataset: item });
  assert.equal(calls[0].command, 'cancel');
  assert.equal(c.S.pendingInstalls.example, undefined);
  assert.equal(c.pendingInstallItems('example').length, 0);
});

test('Big Picture exposes cancellation for a dismissed build choice without an active job', async () => {
  const { c, calls } = context();
  c.dlArgs('example', { id: 'example', target: '/drive/Chosen', tag: 'v2' });
  c.S.setup = {};
  c.S.mods = {};
  c.TV = { view: 'game', arg: 'example' };
  c.GAMEVIEWS = ['game'];
  c.libAppOf = (id) => ({ id, name: 'Example' });
  c.pins = () => [];
  c.mainItem = () => null;
  c.verGroup = () => null;
  c.window.isHidden = () => false;
  c.draw = () => {};
  let items;
  c.sheet = (_title, choices) => { items = choices; c.TV.sheet = {}; };
  vm.runInContext(tv.match(/^  function gameSheet\([^]*?^  }/m)[0], c);
  c.gameSheet('example');
  const cancel = items.find((item) => item.label === 'Cancel the download');
  assert.ok(cancel);
  await cancel.run();
  assert.equal(calls[0].command, 'cancel');
  assert.equal(c.S.dlArgs.example, undefined);
});

test('pending cancellation does not duplicate active or paused download controls', () => {
  const { c } = context();
  c.S.pendingInstalls = { example: { target: '/drive/Chosen' } };
  c.S.jobs.example = { phase: 'download' };
  assert.equal(c.pendingInstallItems('example').length, 0);
  delete c.S.jobs.example;
  c.S.paused.example = { phase: 'paused' };
  assert.equal(c.pendingInstallItems('example').length, 0);
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

async function desktopPreviewContext() {
  const { c, calls } = context();
  const starts = [], pending = [], listeners = {};
  const pathField = { value: '/games/example' }, confirm = { disabled: false }, free = { textContent: '' };
  let modal;
  const invoke = c.invoke;
  c.invoke = (command, args) => {
    if (command === 'install_location' && args.dir) {
      calls.push({ command, args });
      return new Promise((resolve, reject) => pending.push({ resolve, reject }));
    }
    return invoke(command, args);
  };
  c.libChoices = () => '';
  c.$ = (selector) => {
    if (selector === '[data-install-path]') return pathField;
    if (selector === '[data-x="yes"]') return confirm;
    if (selector === '[data-install-free]') return free;
    return { checked: false };
  };
  c.modal = (_html, click) => {
    modal = { dataset: {}, click, addEventListener: (name, fn) => { listeners[name] = fn; } };
    return modal;
  };
  c.startInstall = (...args) => starts.push(args);
  vm.runInContext(fn('locationModal'), c);
  await c.locationModal('example');
  return { c, starts, pending, modal, listeners, pathField, confirm };
}

test('desktop cannot confirm the previous library while the selected library is resolving', async () => {
  const { starts, pending, modal, listeners, pathField, confirm } = await desktopPreviewContext();
  const selection = listeners.change({ target: { name: 'lib', value: '/other-drive' } });
  assert.equal(confirm.disabled, true);
  await modal.click({ dataset: { x: 'yes' } }, () => {});
  assert.equal(starts.length, 0);
  pending[0].resolve({ target: '/other-drive/example', free: 100 });
  await selection;
  assert.equal(confirm.disabled, false);
  assert.equal(pathField.value, '/other-drive/example');
  await modal.click({ dataset: { x: 'yes' } }, () => {});
  assert.equal(starts[0][4], '/other-drive/example');
});

test('an older preview cannot enable confirmation while a newer selection is pending', async () => {
  const { starts, pending, modal, listeners, pathField, confirm } = await desktopPreviewContext();
  const first = listeners.change({ target: { name: 'lib', value: '/first' } });
  const second = listeners.change({ target: { name: 'lib', value: '/second' } });
  pending[0].resolve({ target: '/first/example' });
  await first;
  assert.equal(confirm.disabled, true);
  await modal.click({ dataset: { x: 'yes' } }, () => {});
  assert.equal(starts.length, 0);
  pending[1].resolve({ target: '/second/example' });
  await second;
  assert.equal(pathField.value, '/second/example');
  assert.equal(confirm.disabled, false);
});

test('a failed library preview cannot install into the previous library', async () => {
  const { starts, pending, modal, listeners, confirm } = await desktopPreviewContext();
  const selection = listeners.change({ target: { name: 'lib', value: '/unavailable' } });
  pending[0].reject(new Error('Drive unavailable'));
  await assert.rejects(selection, /Drive unavailable/);
  assert.equal(confirm.disabled, true);
  await modal.click({ dataset: { x: 'yes' } }, () => {});
  assert.equal(starts.length, 0);
});

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
  assert.equal(c.ownFolder({ id: 'example', path: '/games/example', asset: 'game.zip', managed_install: true, preserve_folder: true }), false);
});

test('refresh after uninstall clears cached update arguments before a fresh install', async () => {
  const { c } = context();
  c.S.library.example = { id: 'example', path: '/drive/Chosen' };
  c.S.paused.example = {};
  c.S.prePicked = { example: '/source/game.iso' };
  c.dlArgs('example', { id: 'example', tag: 'old', file: 'old.zip' });
  c.invoke = async () => ({ library: {}, pendingInstalls: {}, settings: {} });
  c.setClock = () => {};
  c.document.body.classList.toggle = () => {};
  c.loadGameSets = async () => {};
  c.S.gameSets = {};
  c.expandLib = () => {};
  c.ENDED = {};
  vm.runInContext(fn('refreshState'), c);
  await c.refreshState();
  assert.equal(c.installArgs('example').tag, null);
  assert.equal(c.installArgs('example').file, null);
  assert.equal(c.S.paused.example, undefined);
  assert.equal(c.S.prePicked.example, undefined);
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

test('the legacy child-game setup folder button opens the parent installation', async () => {
  const { c, calls } = context();
  c.S.library.series = { path: '/drive/Series' };
  c.parentId = (id) => id.split(':')[0];
  vm.runInContext(fn('setupLocationHtml') + fn('act'), c);
  const html = c.setupLocationHtml('series:child', {});
  const id = html.match(/data-act="folder" data-id="([^"]+)"/)[1];
  await c.act({ dataset: { act: 'folder', id } });
  assert.equal(calls[0].command, 'open_folder');
  assert.equal(calls[0].args.id, 'series');
  assert.match(html, /Installed in \/drive\/Series\./);
});

test('the named-input child-game setup folder button opens the parent installation', async () => {
  const { c, calls } = context();
  c.S.library.series = { path: '/drive/Series' };
  c.parentId = (id) => id.split(':')[0];
  vm.runInContext(fn('setupLocationHtml') + fn('act') + named, c);
  const html = c.namedInputsView('series:child', { inputs: [] });
  const id = html.match(/data-act="folder" data-id="([^"]+)"/)[1];
  await c.act({ dataset: { act: 'folder', id } });
  assert.equal(calls[0].command, 'open_folder');
  assert.equal(calls[0].args.id, 'series');
});
