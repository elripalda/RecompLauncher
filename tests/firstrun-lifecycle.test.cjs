const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/firstrun.js'), 'utf8');
function fixture() {
  const classes = new Set(), elements = [], timers = [], renders = [];
  const classList = {
    add: name => classes.add(name), remove: name => classes.delete(name),
    contains: name => classes.has(name),
  };
  let completeSave;
  const context = vm.createContext({
    O: { el: null, step: 0 }, S: { settings: { ui: {} } },
    document: {
      body: { classList, append: el => elements.push(el) },
      createElement: () => ({ setAttribute() {}, addEventListener() {}, classList: { add() {} }, remove() { this.removed = true; } }),
    },
    window: { __TAURI__: { window: { getCurrentWindow: () => ({ setFullscreen() {} }) } } },
    onClick() {}, onKey() {}, draw() {}, render: () => renders.push(classes.has('first-run')),
    save: () => new Promise(resolve => { completeSave = resolve; }),
    setTimeout: fn => timers.push(fn),
  });
  for (const name of ['firstRun', 'finish']) {
    vm.runInContext(source.match(new RegExp(`^  (?:async )?function ${name}\\([^]*?^  }`, 'm'))[0], context);
  }
  return { context, classes, elements, timers, renders, completeSave: () => completeSave() };
}

test('first run hides the underlying launcher for the whole setup session', () => {
  const f = fixture();
  vm.runInContext('firstRun()', f.context);
  assert.equal(f.classes.has('first-run'), true);
  assert.equal(f.elements.length, 1);
  vm.runInContext('firstRun()', f.context);
  assert.equal(f.elements.length, 1);
  assert.equal(f.classes.has('first-run'), true);
});

test('finishing restores the launcher after saving and removes the setup screen', async () => {
  const f = fixture();
  vm.runInContext('firstRun()', f.context);
  const finishing = vm.runInContext('finish()', f.context);
  assert.equal(f.classes.has('first-run'), true);
  f.completeSave(); await finishing;
  assert.equal(f.classes.has('first-run'), false);
  assert.deepEqual(f.renders, [false]);
  assert.equal(f.elements[0].removed, undefined);
  f.timers.forEach(fn => fn());
  assert.equal(f.elements[0].removed, true);
  assert.equal(f.context.O.el, null);
});
