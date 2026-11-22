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

test('prepare.version is in the schema, the developer guide and the profile', () => {
  const schema = JSON.parse(read('docs/recomplauncher.schema.json'));
  const find = (o) => (o && typeof o === 'object' ? (o.keeps_copy && o.options && o.version ? o : Object.values(o).map(find).find(Boolean)) : null);
  assert.equal(find(schema).version.type, 'string');
  assert.match(read('docs/DEVELOPERS.md'), /`"version"` \(optional\)/);
  assert.match(read('src-tauri/src/profile.rs'), /skip_serializing_if = "Vec::is_empty"\)\]\n    pub options: Vec<PrepareOption>/);
});

test("an update doesn't open the game's setup by itself", () => {
  // (Build Games Again After Updates builds in the background; it never opens a window.)
  assert.match(read('ui/app.js'), /else if \(S\.easy\.has\(id\) && wasInstalled\) \{ toast\(tf\('\{name\} is updated\.'/);
});
