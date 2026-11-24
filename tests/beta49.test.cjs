const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..');
const read = (f) => fs.readFileSync(path.join(root, f), 'utf8');

test("no fingerprint: a game is set up on the release its last setup finished on", () => {
  const src = read('src-tauri/src/inputs.rs');
  assert.doesNotMatch(src, /pub fn signature/);
  assert.match(read('src-tauri/src/setup_state.rs'), /f\.set_up == g\.tag/);
  // A setup from an older launcher is read once, at start.
  assert.match(read('src-tauri/src/lib.rs'), /std::thread::spawn\(move \|\| \{\n\s*let st = h\.state::<App>\(\);\n\s*upgrade_setups\(&st\);/);
});

test('prepare.version is out of the schema and the guide: every new release builds again', () => {
  const schema = JSON.parse(read('docs/recomplauncher.schema.json'));
  const find = (o) => (o && typeof o === 'object' ? (o.keeps_copy && o.options && o.done ? o : Object.values(o).map(find).find(Boolean)) : null);
  assert.equal(find(schema).version, undefined);
  assert.doesNotMatch(read('docs/DEVELOPERS.md'), /`"version"` \(optional\)/);
  assert.match(read('docs/DEVELOPERS.md'), /\*\*Every new release builds again\.\*\*/);
  assert.match(read('src-tauri/src/profile.rs'), /skip_serializing_if = "Vec::is_empty"\)\]\n    pub options: Vec<PrepareOption>/);
});

test("an update doesn't open the game's setup by itself", () => {
  // (A game's Build Again After Updates builds in the background; it never opens a window.)
  const app = read('ui/app.js');
  assert.match(app, /else if \(S\.easy\.has\(id\) && wasInstalled\) afterUpdate\(id, name\);/);
  assert.doesNotMatch(app.slice(app.indexOf('async function afterUpdate'), app.indexOf('async function toggleRebuildAfterUpdate')), /setupModal|updateBuildModal/);
});
