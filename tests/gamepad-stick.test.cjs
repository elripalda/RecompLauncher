const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const start = tv.indexOf('  const STD = ');
const end = tv.indexOf('  // F11 switches', start);
assert.ok(start >= 0 && end > start, 'the controller fallback is present');

function controller(settings = {}) {
  let time = 0;
  const timers = [], presses = [];
  const gp = { index: 0, id: 'standard controller', axes: [0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false })) };
  const context = vm.createContext({
    S: { settings }, TV: { on: true }, nativeSeen: false,
    navigator: { getGamepads: () => [gp] }, document: { hasFocus: () => true },
    performance: { now: () => time }, setTimeout: fn => timers.push(fn),
    pad: button => presses.push(button),
  });
  vm.runInContext(tv.slice(start, end), context);
  return {
    gp, settings, presses,
    tick(ms = 33) { time += ms; assert.ok(timers.length); timers.shift()(); },
  };
}

test('disabled stick navigation ignores the fallback controller axes', () => {
  const c = controller({ pad_stick: false });
  c.gp.axes = [0.9, -0.9];
  c.tick();
  assert.deepEqual(c.presses, []);
});

test('the fallback D-pad still works with stick navigation disabled', () => {
  const c = controller({ pad_stick: false });
  c.gp.buttons[13].pressed = true;
  c.tick();
  assert.deepEqual(c.presses, ['down']);
});

test('fallback stick navigation remains enabled by default', () => {
  const c = controller();
  c.gp.axes[0] = -0.9;
  c.tick();
  assert.deepEqual(c.presses, ['left']);
});

test('turning stick navigation off stops a held fallback direction from repeating', () => {
  const c = controller({ pad_stick: true });
  c.gp.axes[0] = 0.9;
  c.tick();
  assert.deepEqual(c.presses, ['right']);
  c.settings.pad_stick = false;
  c.tick(600);
  c.tick(600);
  assert.deepEqual(c.presses, ['right']);
});
