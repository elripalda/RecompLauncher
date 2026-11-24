const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const start = tv.indexOf('  const STD = ');
const end = tv.indexOf('  // F11 switches', start);
assert.ok(start >= 0 && end > start, 'the controller fallback is present');

// The fallback starts once the core has said whether it reads controllers itself (`native`).
async function controller(settings = {}, native = false) {
  let time = 0;
  const timers = [], presses = [];
  const asked = { gamepads: 0 };
  const gp = { index: 0, id: 'standard controller', axes: [0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false })) };
  const getGamepads = () => { asked.gamepads++; return [gp]; };
  const context = vm.createContext({
    S: { settings }, TV: { on: true }, nativeSeen: false, padsAnswer: Promise.resolve(native),
    navigator: { getGamepads }, document: { hasFocus: () => true },
    performance: { now: () => time }, setTimeout: fn => timers.push(fn),
    pad: button => presses.push(button),
  });
  vm.runInContext(tv.slice(start, end), context);
  await new Promise(setImmediate);
  return {
    gp, settings, presses, asked, timers,
    tick(ms = 33) { time += ms; assert.ok(timers.length); timers.shift()(); },
  };
}

test('disabled stick navigation ignores the fallback controller axes', async () => {
  const c = await controller({ pad_stick: false });
  c.gp.axes = [0.9, -0.9];
  c.tick();
  assert.deepEqual(c.presses, []);
});

test('the fallback D-pad still works with stick navigation disabled', async () => {
  const c = await controller({ pad_stick: false });
  c.gp.buttons[13].pressed = true;
  c.tick();
  assert.deepEqual(c.presses, ['down']);
});

test('fallback stick navigation remains enabled by default', async () => {
  const c = await controller();
  c.gp.axes[0] = -0.9;
  c.tick();
  assert.deepEqual(c.presses, ['left']);
});

test('turning stick navigation off stops a held fallback direction from repeating', async () => {
  const c = await controller({ pad_stick: true });
  c.gp.axes[0] = 0.9;
  c.tick();
  assert.deepEqual(c.presses, ['right']);
  c.settings.pad_stick = false;
  c.tick(600);
  c.tick(600);
  assert.deepEqual(c.presses, ['right']);
});

test('the window never asks for gamepads while the core reads controllers', async () => {
  const c = await controller({}, true);
  assert.equal(c.timers.length, 0);
  assert.equal(c.asked.gamepads, 0);
});
