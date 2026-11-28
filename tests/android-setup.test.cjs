const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const profile = fs.readFileSync(path.join(__dirname, '../src-tauri/src/profile.rs'), 'utf8');
const line = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];
const fn = (name) => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

function context(os) {
  const c = vm.createContext({
    S: { os, easy: new Set(['gt3']), cat: { boot_all: true }, library: {}, owned: {}, setup: {}, pc: { vulkan: true, avx2: true, vcredist: true } },
    needsSetup: () => false,
    t: (s) => s, tf: (s) => s, esc: String,
    status: () => 'get', buildWaits: () => false, copyState: () => null, setupStatus: () => null, copyNotNeeded: () => false,
    copyFormats: () => ({ exts: [] }), fmtTags: () => '', copyLine: () => '', pickBtn: () => '', otherFileRows: () => [], systemRows: () => [],
  });
  vm.runInContext(`${line('feedsCopy')}\n${line('turnkey')}\n${fn('readyRows')}\nthis.feedsCopy = feedsCopy; this.turnkey = turnkey;`, c);
  return c;
}
const easy = { id: 'gt3', name: 'GT3 Revisited', origin: 'x360' };
const boots = { id: 'other', name: 'Other', boot: { files: { how: 'copy' } } };

test('On Android no game is set up by the launcher: the game asks for its own copy', () => {
  const c = context('android');
  assert.equal(c.feedsCopy(easy), false);
  assert.equal(c.feedsCopy(boots), false);
  assert.equal(c.turnkey(easy), false);
  // Nothing to check before playing either: no copy to insert, and none of the PC's parts.
  assert.deepEqual(Array.from(c.readyRows(easy)), []);
  // The core has no profile for any game there, so nothing offers Easy Setup or Game Settings.
  assert.match(profile, /pub fn find\(game_dir: &Path, tag: &str, entry: &Value\) -> Option<Profile> \{[^}]*if cfg!\(target_os = "android"\) \{\s*return None;/);
});

test('On a PC Easy Setup games are still set up by the launcher', () => {
  const c = context('windows');
  assert.equal(c.feedsCopy(easy), true);
  // A boot profile alone is the core's to count: the window never guesses from it.
  assert.equal(c.feedsCopy(boots), false);
  assert.equal(c.turnkey(easy), true);
  const rows = Array.from(c.readyRows(easy)).map((r) => r.k);
  assert.ok(rows.includes('Your copy of the game'));
  assert.ok(rows.includes('Processor (AVX2)'));
});
