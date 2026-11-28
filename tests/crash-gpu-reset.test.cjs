const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

// The crash window's "your graphics driver stopped responding" check, as app.js has it.
function gpuReset() {
  const line = app.split('\n').find((l) => l.includes('const gpuReset = '));
  assert.ok(line);
  const literal = line.match(/&& (\/.*\/[a-z]*)\.test\(said\)/)[1];
  return new Function(`return ${literal};`)();
}

test('Vulkan\'s own error name for a lost device is a driver reset', () => {
  assert.ok(gpuReset().test('VK_ERROR_DEVICE_LOST'));
});

test('the C++ Vulkan wrapper\'s name for a lost device is a driver reset', () => {
  assert.ok(gpuReset().test('Device lost during submit: vk::Queue::submit: ErrorDeviceLost'));
});

test('a Vulkan log line saying the device was lost is a driver reset', () => {
  assert.ok(gpuReset().test('[host] vk: device lost: heap 0 usage 512 MB'));
});

test('a lost audio device is not a driver reset', () => {
  assert.ok(!gpuReset().test('audio: device lost, reopening'));
});
