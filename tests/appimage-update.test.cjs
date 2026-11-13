const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const { spawn } = require('node:child_process');
const { once } = require('node:events');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const more = fs.readFileSync(path.join(__dirname, '../ui/more.js'), 'utf8');
const helper = fs.readFileSync(path.join(__dirname, '../src-tauri/src/appimage_update.sh'), 'utf8');
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test('Linux update passes the current mode, ignores duplicate clicks, and allows retry', async () => {
  for (const big of [true, false]) {
    const calls = [];
    let reject;
    const ctx = vm.createContext({
      S: { os: 'linux' }, document: { body: { classList: { contains: () => big } } },
      invoke: (...args) => { calls.push(args); return new Promise((_, no) => { reject = no; }); },
      toast() {}, t: (s) => s, blocked: () => false, cantReplace: () => false, fail() {}, lookForLauncher() {},
    });
    vm.runInContext(app.match(/^async function selfUpdate\([^]*?^}/m)[0], ctx);
    const first = ctx.selfUpdate();
    await ctx.selfUpdate();
    assert.equal(calls.length, 1);
    assert.equal(calls[0][0], 'update_launcher');
    assert.equal(calls[0][1].bigPicture, big);
    reject(new Error('download failed'));
    await first;
    assert.equal(ctx.S.selfUpdating, false);
    const retry = ctx.selfUpdate();
    assert.equal(calls.length, 2);
    reject(new Error('retry failed'));
    await retry;
  }
});

test('Windows and macOS retain their original update invocation', async () => {
  for (const osName of ['windows', 'macos']) {
    const calls = [];
    const ctx = vm.createContext({ S: { os: osName }, toast() {}, t: (s) => s, invoke: async (...args) => calls.push(args) });
    vm.runInContext(app.match(/^async function selfUpdate\([^]*?^}/m)[0], ctx);
    await ctx.selfUpdate();
    assert.deepEqual(calls, [['update_launcher']]);
  }
});

test('desktop restart overrides the saved Big Picture preference', () => {
  const ctx = vm.createContext({ S: { desktopArg: true, bigArg: false, settings: { start_in_tv: true } } });
  vm.runInContext(app.match(/^function startsInTV\(.*$/m)[0], ctx);
  assert.equal(ctx.startsInTV(), false);
  ctx.S.desktopArg = false;
  assert.equal(ctx.startsInTV(), true);
});

test('Linux progress stays open and failure removes it before retry', () => {
  let progress;
  const boxes = [];
  const ctx = vm.createContext({
    S: { os: 'linux' }, T: { event: { listen: (_, cb) => { progress = cb; } } },
    modal: () => { const box = { isConnected: true, dataset: {}, remove() { this.isConnected = false; } }; boxes.push(box); return box; },
    $: () => null, esc: (s) => s, t: (s) => s,
  });
  vm.runInContext(more.slice(more.indexOf('let updBox = null;'), more.indexOf('// What\'s new:')), ctx);
  progress({ payload: { done: 0, total: 10 } });
  assert.equal(boxes[0].dataset.busy, 'true');
  progress({ payload: { failed: true } });
  assert.equal(boxes[0].isConnected, false);
  progress({ payload: { done: 0, total: 10 } });
  assert.equal(boxes.length, 2);
});

async function fixture(t, { next, parent = '2147483647', source = helper, reopen = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rl-linux-update-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  // Shell metacharacters, Unicode and spaces must remain literal paths.
  const target = path.join(dir, "Launcher α ' ; $unused.AppImage");
  const work = path.join(dir, 'private staging');
  const trail = path.join(dir, 'launcher-trail.log');
  const marker = path.join(dir, 'started.txt');
  fs.mkdirSync(work, { mode: 0o700 });
  const old = '#!/bin/sh\nprintf "old:%s" "$1" > "$RL_TEST_MARKER"\n';
  fs.writeFileSync(target, old, { mode: 0o755 });
  fs.linkSync(target, path.join(work, 'previous.AppImage'));
  if (next !== null) fs.writeFileSync(path.join(work, 'next.AppImage'), next || '#!/bin/sh\nprintf "new:%s" "$1" > "$RL_TEST_MARKER"\n', { mode: 0o755 });
  const child = spawn('/bin/sh', ['-c', source, 'update-test', parent, target, work, trail, reopen ? '1' : '0', '--bigpicture'], {
    env: { ...process.env, RL_TEST_MARKER: marker }, stdio: 'ignore',
  });
  t.after(() => { if (child.exitCode === null) child.kill(); });
  const exited = once(child, 'exit');
  return { dir, target, work, trail, marker, exited, old };
}

test('AppImage helper waits for the old process, then replaces the same path and starts it', { skip: process.platform !== 'linux' }, async (t) => {
  const parent = spawn('/bin/sleep', ['30']);
  const parentExited = once(parent, 'exit');
  t.after(() => parent.kill());
  const f = await fixture(t, { parent: String(parent.pid) });
  await pause(250);
  assert.equal(fs.readFileSync(f.target, 'utf8'), f.old);
  assert.equal(fs.existsSync(f.marker), false);
  parent.kill();
  await parentExited;
  assert.equal((await f.exited)[0], 0);
  assert.equal(fs.readFileSync(f.marker, 'utf8'), 'new:--bigpicture');
  assert.equal(fs.statSync(f.target).mode & 0o777, 0o755);
  assert.equal(fs.existsSync(f.work), false);
  assert.equal(fs.existsSync(f.trail), false);
});

test('AppImage helper restores and reopens the previous image on immediate launch failure', { skip: process.platform !== 'linux' }, async (t) => {
  const f = await fixture(t, { next: '#!/bin/sh\nexit 7\n' });
  assert.equal((await f.exited)[0], 0);
  assert.equal(fs.readFileSync(f.target, 'utf8'), f.old);
  for (let i = 0; i < 30 && !fs.existsSync(f.marker); i++) await pause(10);
  assert.equal(fs.readFileSync(f.marker, 'utf8'), 'old:--bigpicture');
  assert.match(fs.readFileSync(f.trail, 'utf8'), /previous version was restored/);
});

test('AppImage helper replaces the image without reopening a window closed to the tray', { skip: process.platform !== 'linux' }, async (t) => {
  const f = await fixture(t, { reopen: false });
  assert.equal((await f.exited)[0], 0);
  assert.notEqual(fs.readFileSync(f.target, 'utf8'), f.old);
  assert.equal(fs.existsSync(f.marker), false);
  assert.equal(fs.existsSync(f.work), false);
  assert.equal(fs.existsSync(f.trail), false);
});

test('AppImage helper cleans staging while a successful replacement keeps running', { skip: process.platform !== 'linux' }, async (t) => {
  const f = await fixture(t, { next: '#!/bin/sh\nprintf "%s" "$$" > "$RL_TEST_MARKER"\nexec /bin/sleep 30\n' });
  let pid;
  t.after(() => {
    if (pid) { try { process.kill(pid); } catch {} }
  });
  assert.equal((await f.exited)[0], 0);
  pid = Number(fs.readFileSync(f.marker, 'utf8'));
  assert.doesNotThrow(() => process.kill(pid, 0));
  assert.equal(fs.existsSync(f.work), false);
  assert.equal(fs.existsSync(f.trail), false);
});

test('AppImage helper leaves the original intact when replacement fails', { skip: process.platform !== 'linux' }, async (t) => {
  const f = await fixture(t, { next: null });
  assert.equal((await f.exited)[0], 1);
  assert.equal(fs.readFileSync(f.target, 'utf8'), f.old);
  assert.match(fs.readFileSync(f.trail, 'utf8'), /installed launcher hasn't changed/);
  assert.equal(fs.existsSync(f.work), false);
  // The helper reopens the unchanged launcher in the background; let it write its marker before
  // the folder is removed, or the removal can race it and fail.
  for (let i = 0; i < 30 && !fs.existsSync(f.marker); i++) await pause(10);
});

test('AppImage helper reopens the unchanged launcher when replacement fails', { skip: process.platform !== 'linux' }, async (t) => {
  const f = await fixture(t, { next: null });
  await f.exited;
  for (let i = 0; i < 30 && !fs.existsSync(f.marker); i++) await pause(10);
  assert.equal(fs.readFileSync(f.marker, 'utf8'), 'old:--bigpicture');
});

test('A folder the AppImage cannot replace itself in gets a window, not a bare error', () => {
  const opened = [];
  let shown = '';
  const ctx = vm.createContext({
    S: { launcherUpd: { url: 'https://github.com/o/r/new.AppImage' }, site: 'https://recomplauncher.com' },
    t: (s) => s, tf: (s, v) => s.replace('{folder}', v.folder), esc: (s) => s, fail() {},
    modal: (html, onClick) => { shown = html; onClick({ dataset: { x: 'go' } }, () => {}); },
    invoke: (cmd, args) => { opened.push([cmd, args.url]); return Promise.resolve(); },
  });
  vm.runInContext(app.match(/^function cantReplace\([^]*?^}/m)[0], ctx);
  assert.equal(ctx.cantReplace(new Error('Something else')), false);
  assert.equal(ctx.cantReplace('CANT_REPLACE|/opt/Recomp Launcher'), true);
  assert.match(shown, /\/opt\/Recomp Launcher/);
  assert.deepEqual(opened, [['open_url', 'https://github.com/o/r/new.AppImage']]);
});

test('A Linux update that did not go in offers to try again', () => {
  let shown = '';
  let retried = 0;
  const ctx = vm.createContext({
    S: { os: 'linux', version: '1.0' }, document: { body: { classList: { contains: () => false } } },
    $: () => null, t: (s) => s, tf: (s) => s, esc: (s) => s, verName: (v) => v,
    modal: (html, onClick) => { shown = html; onClick({ dataset: { x: 'go' } }, () => {}); },
    selfUpdate: () => { retried++; },
  });
  vm.runInContext(app.match(/^function updateFailedModal\([^]*?^}/m)[0], ctx);
  ctx.updateFailedModal({ to: '2.0', bad: false });
  assert.match(shown, /The Update Didn't Start/);
  assert.equal(retried, 1);
});

test('AppImage helper gives up without replacing a launcher that does not exit', { skip: process.platform !== 'linux' }, async (t) => {
  const f = await fixture(t, { parent: String(process.pid), source: helper.replace('"$tries" -ge 60', '"$tries" -ge 1') });
  assert.equal((await f.exited)[0], 1);
  assert.equal(fs.readFileSync(f.target, 'utf8'), f.old);
  assert.match(fs.readFileSync(f.trail, 'utf8'), /left unapplied/);
  assert.equal(fs.existsSync(f.work), false);
});
