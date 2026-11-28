const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const line = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];

// The launcher's own rule, run against a made-up Library: catalog games a, b and c.
function staysInLibrary(ui) {
  const ctx = { S: { settings: { ui } }, byId: (id) => (['a', 'b', 'c'].includes(id) ? { id } : null) };
  const src = [line('saved'), line('isSaved'), line('foundOnly'), line('staysInLibrary'), 'this.stays = staysInLibrary;'].join('\n');
  vm.runInNewContext(src, ctx);
  return ctx.stays;
}

test('a game the player added stays in the Library after Uninstall', () => {
  assert.equal(staysInLibrary({ saved: { a: 1 } })('a'), true);
});

test('a game only a scan found leaves the Library with Uninstall', () => {
  assert.equal(staysInLibrary({ saved: { b: 1 }, found_saved: ['b'] })('b'), false);
});

test('a game that was never in the saved list leaves the Library with Uninstall', () => {
  assert.equal(staysInLibrary({ saved: {} })('c'), false);
});

test('a game no longer in the catalog leaves the Library with Uninstall', () => {
  assert.equal(staysInLibrary({ saved: { z: 1 } })('z'), false);
});

test('the Uninstall window says what will happen, from the same rule', () => {
  assert.match(app, /const stays = staysInLibrary\(id\);/);
  assert.match(app, /stays \? t\('It stays in your Library under Not Installed, so you can install it again any time\.'\) : t\('It comes out of your Library\./);
});

test('the Manage tab and Big Picture say it from the same rule', () => {
  assert.match(app, /staysInLibrary\(id\) \? t\('It stays in your Library under Not Installed\. You choose whether its folder goes too\.'\)/);
  assert.match(tv, /staysInLibrary\(id\) \? t\('Its files stay, and it stays in your Library under Not Installed'\)/);
});

test('a game only a scan found leaves the saved list as soon as it is uninstalled', () => {
  const keepOut = app.match(/^async function keepOut\(id\) \{[\s\S]*?^\}/m)[0];
  assert.match(keepOut, /const found = foundOnly\(id\);/);
  assert.match(keepOut, /delete s\[id\];/);
});
