const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const { spawn } = require('node:child_process');
const { once } = require('node:events');

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const helper = fs.readFileSync(path.join(__dirname, '../src-tauri/src/self_update.sh'), 'utf8');

// Run the embedded commands with test doubles for LaunchServices and codesign. This exercises
// real directory swaps without opening any installed app or launching a downloaded executable.
// Rust tests separately exercise codesign against real signed application bundles.
function fixture(t, newVersion = 'v2') {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rl-mac-update-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const target = path.join(dir, 'Recomp Launcher $(touch injected) — test.app');
  const swap = path.join(dir, 'adjacent staging');
  const data = path.join(dir, 'app data staging');
  const next = path.join(swap, 'RecompLauncher.app');
  const marker = path.join(dir, 'opened');
  const error = path.join(dir, 'error');
  fs.mkdirSync(target);
  fs.mkdirSync(swap);
  fs.mkdirSync(data);
  fs.mkdirSync(next);
  fs.writeFileSync(path.join(target, 'version'), 'v1\n');
  fs.writeFileSync(path.join(target, 'obsolete'), 'old bundle only');
  fs.writeFileSync(path.join(next, 'version'), newVersion + '\n');
  fs.writeFileSync(path.join(next, 'new-resource'), 'new bundle only');
  const opener = path.join(dir, 'test opener');
  const signature = path.join(dir, 'test signature');
  fs.writeFileSync(opener, '#!/bin/sh\nIFS= read -r version < "$2/version"\n[ "$version" != bad ] || exit 17\nprintf "%s\\n" "$version" "$@" > "$TEST_MARKER"\n', { mode: 0o755 });
  fs.writeFileSync(signature, '#!/bin/sh\nfor last; do :; done\n[ ! -f "$last/invalid-signature" ]\n', { mode: 0o755 });
  function run(pid, failRestore = false) {
    let source = "printf 'ready\\n' >&3\n" + helper.replace('/usr/bin/open', '"$TEST_OPENER"').replace('/usr/bin/codesign', '"$TEST_CODESIGN"');
    const mover = path.join(dir, 'test mover');
    if (failRestore) {
      fs.writeFileSync(mover, '#!/bin/sh\n[ "$1" != "$TEST_PREVIOUS" ] || exit 23\n/bin/mv "$@"\n', { mode: 0o755 });
      source = source.replaceAll('/bin/mv', '"$TEST_MOVER"');
    }
    const child = spawn('/bin/sh', ['-c', source, 'mac-update-test', String(pid), target, next, swap, data, error, '--bigpicture'], {
      cwd: dir,
      env: { ...process.env, TEST_OPENER: opener, TEST_CODESIGN: signature, TEST_MARKER: marker, TEST_MOVER: mover, TEST_PREVIOUS: path.join(swap, 'previous.app') },
      stdio: ['ignore', 'pipe', 'pipe', 'pipe'],
    });
    t.after(() => { if (child.exitCode === null) child.kill(); });
    let stderr = '';
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    return { child, ready: once(child.stdio[3], 'data'), done: once(child, 'exit'), stderr: () => stderr };
  }
  return { dir, target, swap, data, next, marker, error, run };
}

async function exitedPid() {
  const child = spawn('/bin/sh', ['-c', 'exit 0']);
  const pid = child.pid;
  await once(child, 'exit');
  return pid;
}

test('Mac swap waits for shutdown, replaces the complete bundle and relaunches in Big Picture', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t);
  const parent = spawn('/bin/sleep', ['30']);
  t.after(() => { if (parent.exitCode === null) parent.kill(); });
  const parentDone = once(parent, 'exit');
  const run = f.run(parent.pid);
  await run.ready;
  await pause(100);
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
  assert.equal(fs.existsSync(f.marker), false);
  parent.kill();
  await parentDone;
  const [code] = await run.done;
  assert.equal(code, 0, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v2\n');
  assert.equal(fs.existsSync(path.join(f.target, 'obsolete')), false);
  assert.equal(fs.existsSync(path.join(f.target, 'new-resource')), true);
  assert.match(fs.readFileSync(f.marker, 'utf8'), /^v2\n/);
  assert.match(fs.readFileSync(f.marker, 'utf8'), /--bigpicture/);
  assert.equal(fs.existsSync(f.swap), false);
  assert.equal(fs.existsSync(f.data), false);
  assert.equal(fs.existsSync(f.error), false);
  assert.equal(fs.existsSync(path.join(f.dir, 'injected')), false);
});

test('a failed Mac relaunch restores the complete previous bundle and cleans staging', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'bad');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
  assert.equal(fs.existsSync(path.join(f.target, 'obsolete')), true);
  assert.equal(fs.existsSync(path.join(f.target, 'new-resource')), false);
  assert.match(fs.readFileSync(f.marker, 'utf8'), /^v1\n/);
  assert.match(fs.readFileSync(f.error, 'utf8'), /Restoring the previous version/);
  assert.equal(fs.existsSync(f.swap), false);
  assert.equal(fs.existsSync(f.data), false);
});

test('Mac signature failure after shutdown leaves the installed app intact and cleans staging', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.next, 'invalid-signature'), '');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
  assert.match(fs.readFileSync(f.error, 'utf8'), /invalid signature/);
  assert.equal(fs.existsSync(f.swap), false);
  assert.equal(fs.existsSync(f.data), false);
});

test('failed Mac restoration retains the previous bundle but removes the download', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'bad');
  const run = f.run(await exitedPid(), true);
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  const previous = path.join(f.swap, 'previous.app');
  assert.equal(fs.readFileSync(path.join(previous, 'version'), 'utf8'), 'v1\n');
  assert.equal(fs.existsSync(f.data), false);
  assert.ok(fs.readFileSync(f.error, 'utf8').includes(previous));
});

function updateContext(osName, invoke) {
  const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
  const implementation = source.match(/^async function selfUpdate\([^]*?^}/m)[0];
  const context = vm.createContext({
    S: { os: osName },
    document: { body: { classList: { contains: () => true } } },
    toast() {}, t: (s) => s, blocked: () => false, fail() {}, lookForLauncher() {}, invoke,
  });
  vm.runInContext(implementation, context);
  return context;
}

test('repeated Mac update clicks make one request and a failure allows retry', async () => {
  let finish;
  const calls = [];
  const context = updateContext('macos', (...args) => {
    calls.push(args);
    return new Promise((resolve, reject) => { finish = { resolve, reject }; });
  });
  const first = context.selfUpdate();
  await context.selfUpdate();
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], 'update_launcher');
  assert.equal(calls[0][1].bigPicture, true);
  finish.reject('download failed');
  await first;
  assert.equal(context.S.selfUpdating, false);
  const retry = context.selfUpdate();
  assert.equal(calls.length, 2);
  finish.resolve();
  await retry;
});

test('Windows and Linux keep the original update command and click behavior', async () => {
  for (const osName of ['windows', 'linux']) {
    const calls = [];
    const context = updateContext(osName, async (...args) => calls.push(args));
    context.S.selfUpdating = true;
    await context.selfUpdate();
    assert.deepEqual(calls, [['update_launcher']]);
    assert.equal(context.S.selfUpdating, true);
  }
});

test('only the Mac progress dialog is held open, and a failed update closes it', () => {
  const source = fs.readFileSync(path.join(__dirname, '../ui/more.js'), 'utf8').split("// What's new:")[0];
  for (const osName of ['macos', 'windows', 'linux']) {
    const listeners = new Map();
    let removed = false;
    const box = { isConnected: true, dataset: {}, remove() { removed = true; } };
    const bar = { style: {} }, status = {};
    const context = vm.createContext({
      S: { os: osName },
      T: { event: { listen: (name, handler) => listeners.set(name, handler) } },
      modal: () => box,
      t: (s) => s, esc: (s) => s, tf: (s) => s, fmtB: (s) => s,
      $: (selector) => selector === '.track i' ? bar : status,
    });
    vm.runInContext(source, context);
    const progress = listeners.get('self-update');
    progress({ payload: { done: 50, total: 100 } });
    assert.equal(box.dataset.busy, osName === 'macos' ? 'true' : undefined);
    assert.equal(bar.style.width, '50%');
    if (osName === 'macos') {
      progress({ payload: { failed: true } });
      assert.equal(removed, true);
    }
  }
});
