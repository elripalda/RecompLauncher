const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

// Run the production savePinOrder with the saved pins it starts from, and see what it saves.
function save(pinned, shown) {
  const saved = [];
  let drawn = 0;
  const context = vm.createContext({
    S: { settings: { ui: { pins: pinned } } },
    saveUi: (patch) => saved.push(patch),
    renderSide: () => drawn++,
  });
  vm.runInContext(source.match(/^function savePinOrder\([^]*?^}/m)[0], context);
  context.savePinOrder(shown);
  return { saved: JSON.parse(JSON.stringify(saved)), drawn };
}

test('a moved pin is saved in its new place', () => {
  const { saved } = save(['a', 'b', 'c'], ['c', 'a', 'b']);
  assert.deepEqual(saved, [{ pins: ['c', 'a', 'b'] }]);
});

test('a pin the sidebar could not show stays saved at the end', () => {
  const { saved } = save(['a', 'gone', 'b'], ['b', 'a']);
  assert.deepEqual(saved, [{ pins: ['b', 'a', 'gone'] }]);
});

test('the sidebar is drawn again after a move', () => {
  const { drawn } = save(['a', 'b'], ['b', 'a']);
  assert.equal(drawn, 1);
});

test('the pinned games listen for a drag and for Alt with the arrow keys', () => {
  const setup = source.match(/^function pinDrag\([^]*?^}/m)[0];
  for (const name of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', 'keydown']) {
    assert.match(setup, new RegExp(`addEventListener\\('${name}'`));
  }
  assert.match(source, /^pinDrag\(\);$/m);
});
