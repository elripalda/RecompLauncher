const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const DRIVER = 'Your graphics driver stopped responding';
const PROCESSOR = 'your processor doesn\'t have an instruction it needs';

// The crash window for one exit, with the game's log: what it says, and the code it gives
// Troubleshoot (shown there as how the game closed, and in the reports).
async function crashWindow(code, secs, log) {
  let html = '';
  let onClick = null;
  let shown = null;
  const c = vm.createContext({
    S: { library: { g: { name: 'Game', tag: 'v1' } }, settings: { ui: {} }, setup: {}, os: 'windows', version: '1.0.0' },
    OS_NAME: { windows: 'Windows' },
    invoke: async (cmd) => (cmd === 'recent_log' ? ['game.log', log.join('\n')] : null),
    modal: (h, click) => { html = h; onClick = click; return {}; },
    troubleshootModal: (id, lastCode) => { shown = lastCode; },
    byId: () => null, isServer: () => false, bootNote: () => '', toast() {}, fail() {},
    N_: (s) => s, t: (s) => s, tf: (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => v[k]), esc: (s) => String(s),
  });
  const code_ = ['codeMeaning', 'evenActs', 'knownFix', 'badVersion', 'updateBroke', 'crashModal'].map(fn).join('\n');
  vm.runInContext(`${app.match(/^const verKey = .*$/m)[0]}\n${code_}`, c);
  await c.crashModal({ id: 'g', code, secs, started: 0 });
  await onClick({ dataset: { x: 'trouble' } }, () => {});
  return { html, shown };
}

// Age of Decomp in #601: its Vulkan device was lost, and its log mentions memory commit too.
const decomp = [
  '[host] mem: vram frame 12000: vma 17 blocks 7077 MiB, 1111 allocations 6843 MiB; textures live 1082 (own memory 1082, 1316 MiB), released 8 (own memory 8, 6 MiB); mirrors 2 5351 MiB; releases pending 0',
  '[host] vk: device lost: heap 0 usage 5784 MiB of budget 15156 MiB (VMA blocks 48 MiB)',
  '[host] vk: device lost: heap 1 usage 7260 MiB of budget 15419 MiB (VMA blocks 7077 MiB)',
  '[host] vk: device fault: "Device lost detected" (0 address infos, 0 vendor infos, vendor binary 0 bytes)',
  '[host] fatal: memory commit: this process 18282 MB, the system 45278 of 53353 MB',
];
// DekUI (Azahar) in #615: it lost the device 70 s in and stopped itself with 0xC000001D.
const azahar = [
  '[  69.958408] Debug <Critical> video_core/renderer_vulkan/vk_master_semaphore.cpp:operator():101: Unreachable code!',
  'Device lost during submit: vk::Queue::submit: ErrorDeviceLost',
];

test('a lost Vulkan device with memory lines in the log is a driver reset, not memory', async () => {
  const w = await crashWindow(70, 242, decomp);
  assert.ok(w.html.includes(DRIVER));
  assert.ok(!w.html.includes('Your PC ran out of memory'));
});

test('a lost device then 0xC000001D after 75 s says nothing about the processor', async () => {
  const w = await crashWindow(-1073741795, 75, azahar);
  assert.ok(w.html.includes(DRIVER));
  assert.ok(!w.html.includes(PROCESSOR));
  assert.equal(w.shown, '-1073741795');
});

test('0xC000001D at start with nothing in the log still means the processor', async () => {
  const w = await crashWindow(-1073741795, 1, ['Starting…']);
  assert.ok(!w.html.includes(DRIVER));
  assert.ok(w.html.includes(PROCESSOR));
  assert.equal(w.shown, `-1073741795: ${PROCESSOR}`);
});

test('a lost audio device is not a driver reset', async () => {
  const w = await crashWindow(1, 40, ['audio: device lost, reopening']);
  assert.ok(!w.html.includes(DRIVER));
});
