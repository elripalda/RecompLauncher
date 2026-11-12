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
async function until(check) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (check()) return;
    await pause(10);
  }
  assert.fail('test replacement did not start');
}

// A small executable represents the new app, with explicit readiness, crash and hang modes.
// Only recovery uses the LaunchServices double. No installed or downloaded app is launched.
function fixture(t, newVersion = 'v2', startup = 'ready') {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rl-mac-update-'));
  const target = path.join(dir, 'Recomp Launcher $(touch injected) — test.app');
  const swap = path.join(dir, 'adjacent staging');
  const data = path.join(dir, 'app data staging');
  const next = path.join(swap, 'RecompLauncher.app');
  const marker = path.join(dir, 'opened');
  const pidFile = path.join(dir, 'replacement.pid');
  const error = path.join(dir, 'error');
  const token = 'a'.repeat(64);
  const program = path.join(target, 'launcher');
  t.after(() => {
    if (fs.existsSync(pidFile)) {
      try { process.kill(Number(fs.readFileSync(pidFile, 'utf8'))); } catch {}
    }
    fs.rmSync(dir, { recursive: true, force: true });
  });
  fs.mkdirSync(target);
  fs.mkdirSync(swap);
  fs.mkdirSync(data);
  fs.mkdirSync(next);
  fs.writeFileSync(path.join(target, 'version'), 'v1\n');
  fs.writeFileSync(path.join(target, 'obsolete'), 'old bundle only');
  fs.writeFileSync(path.join(next, 'version'), newVersion + '\n');
  fs.writeFileSync(path.join(next, 'new-resource'), 'new bundle only');
  const programSource = [
    '#!/bin/sh',
    'printf "%s\\n" "$$" > "$TEST_PID_FILE"',
    'printf "%s\\n" "$TEST_VERSION" "$@" > "$TEST_MARKER"',
    '[ "$TEST_VERSION" != bad ] || exit 17',
    '[ "$TEST_STARTUP" != exit-ok ] || exit 0',
    'if [ "$TEST_STARTUP" = hang ]; then trap "" TERM; else trap "exit 0" TERM; fi',
    'case "$TEST_STARTUP" in',
    '  ready|ack-exit) printf "%s %s\\n" "$$" "$4" > "$2/startup.ready" ;;',
    '  wrong-token) printf "%s invalid-token\\n" "$$" > "$2/startup.ready" ;;',
    '  wrong-pid) printf "1 %s\\n" "$4" > "$2/startup.ready" ;;',
    'esac',
    '[ "$TEST_STARTUP" != ack-exit ] || exit 0',
    'while :; do /bin/sleep 0.05; done',
    '',
  ].join('\n');
  fs.writeFileSync(path.join(next, 'launcher'), programSource, { mode: 0o755 });
  // The installed (old) binary performs the exchange; a shell double stands in for renamex_np.
  const exchangeSource = [
    '#!/bin/sh',
    '[ "$1" = --exchange-bundles ] || exit 64',
    '[ -z "$TEST_FAIL_RESTORE" ] || [ ! -e "$TEST_EXCHANGED" ] || exit 23',
    ': > "$TEST_EXCHANGED"',
    '/bin/mv "$2" "$2.exchange" && /bin/mv "$3" "$2" && /bin/mv "$2.exchange" "$3"',
    '',
  ].join('\n');
  fs.writeFileSync(path.join(target, 'launcher'), exchangeSource, { mode: 0o755 });
  const opener = path.join(dir, 'test opener');
  const signature = path.join(dir, 'test signature');
  fs.writeFileSync(opener, '#!/bin/sh\nIFS= read -r version < "$2/version"\nprintf "%s\\n" "$version" "$@" > "$TEST_MARKER"\n', { mode: 0o755 });
  fs.writeFileSync(signature, '#!/bin/sh\nfor last; do :; done\n[ ! -f "$last/invalid-signature" ]\n', { mode: 0o755 });
  function run(pid, failRestore = false) {
    let source = "printf 'ready\\n' >&3\n" + helper.replace('/usr/bin/open', '"$TEST_OPENER"').replace('/usr/bin/codesign', '"$TEST_CODESIGN"');
    source = source.replace('startup_tries=60', 'startup_tries=30').replaceAll('/bin/sleep 1', '/bin/sleep 0.05');
    const child = spawn('/bin/sh', ['-c', source, 'mac-update-test', String(pid), target, next, swap, data, error, program, token, '--bigpicture'], {
      cwd: dir,
      env: { ...process.env, TEST_OPENER: opener, TEST_CODESIGN: signature, TEST_MARKER: marker, TEST_EXCHANGED: path.join(dir, 'exchanged'), TEST_FAIL_RESTORE: failRestore ? '1' : '', TEST_PID_FILE: pidFile, TEST_VERSION: newVersion, TEST_STARTUP: startup },
      stdio: ['ignore', 'pipe', 'pipe', 'pipe'],
    });
    t.after(() => { if (child.exitCode === null) child.kill(); });
    let stderr = '';
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    return { child, ready: once(child.stdio[3], 'data'), done: once(child, 'exit'), stderr: () => stderr };
  }
  function acknowledge() {
    const pid = fs.readFileSync(pidFile, 'utf8').trim();
    fs.writeFileSync(path.join(swap, 'startup.ready'), pid + ' ' + token + '\n');
  }
  return { dir, target, swap, data, next, marker, pidFile, error, run, acknowledge };
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

test('the previous bundle remains until the replacement acknowledges startup', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'v2', 'manual');
  const run = f.run(await exitedPid());
  await until(() => fs.existsSync(f.pidFile));
  await pause(100);
  assert.equal(run.child.exitCode, null);
  assert.equal(fs.existsSync(path.join(f.swap, 'RecompLauncher.app', 'obsolete')), true);
  assert.equal(fs.existsSync(f.data), true);
  f.acknowledge();
  const [code] = await run.done;
  assert.equal(code, 0, run.stderr());
  assert.equal(fs.existsSync(f.swap), false);
  assert.equal(fs.existsSync(f.data), false);
});

test('exit zero without a readiness signal rolls back', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'v2', 'exit-ok');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
});

test('a crash immediately after sending readiness still rolls back', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'v2', 'ack-exit');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
});

test('a hung replacement is killed before the previous bundle is restored', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'v2', 'hang');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
  const pid = Number(fs.readFileSync(f.pidFile, 'utf8'));
  assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' });
  assert.equal(fs.existsSync(f.swap), false);
});

test('a readiness signal with the wrong challenge cannot discard the backup', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'v2', 'wrong-token');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
});

test('a readiness signal from another PID cannot discard the backup', { skip: process.platform === 'win32' }, async (t) => {
  const f = fixture(t, 'v2', 'wrong-pid');
  const run = f.run(await exitedPid());
  const [code] = await run.done;
  assert.equal(code, 1, run.stderr());
  assert.equal(fs.readFileSync(path.join(f.target, 'version'), 'utf8'), 'v1\n');
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
  const previous = path.join(f.swap, 'RecompLauncher.app');
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

test('startup is acknowledged only after state loaded and two UI frames rendered', async () => {
  const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
  const implementation = source.match(/^async function confirmLauncherStartup\([^]*?^}/m)[0];
  const frames = [];
  const calls = [];
  const context = vm.createContext({
    S: { os: 'macos' },
    requestAnimationFrame: (callback) => frames.push(callback),
    invoke: async (command) => calls.push(command),
  });
  vm.runInContext(implementation, context);
  const ready = context.confirmLauncherStartup(true);
  assert.equal(calls.length, 0);
  frames.shift()();
  assert.equal(calls.length, 0);
  frames.shift()();
  await ready;
  assert.deepEqual(calls, ['launcher_ready']);
});

test('failed startup and non-Mac startup cannot send an acknowledgment', async () => {
  const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
  const implementation = source.match(/^async function confirmLauncherStartup\([^]*?^}/m)[0];
  const calls = [];
  const context = vm.createContext({
    S: { os: 'macos' },
    requestAnimationFrame: () => assert.fail('no readiness frame should be scheduled'),
    invoke: async (command) => calls.push(command),
  });
  vm.runInContext(implementation, context);
  await context.confirmLauncherStartup(false);
  context.S.os = 'windows';
  await context.confirmLauncherStartup(true);
  context.S.os = 'linux';
  await context.confirmLauncherStartup(true);
  assert.equal(calls.length, 0);
});

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

test('Big Picture B does not dismiss a dialog that is busy', () => {
  const source = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
  assert.match(source, /if \(name === 'b'\) \{ if \(!dlg\.dataset\.busy\) dlg\.remove\(\); \}/);
});

test('the desktop flag from a Mac update beats the saved Big Picture setting', () => {
  const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
  const implementation = source.match(/^function startsInTV\(\)[^\n]*/m)[0];
  const startsInTV = (S) => vm.runInNewContext(implementation + '; startsInTV()', { S });
  assert.equal(startsInTV({ settings: { start_in_tv: true }, desktopArg: true }), false);
  assert.equal(startsInTV({ settings: { start_in_tv: true } }), true);
  assert.equal(startsInTV({ settings: {}, bigArg: true }), true);
});
