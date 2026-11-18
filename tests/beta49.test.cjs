const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..');
const read = (f) => fs.readFileSync(path.join(root, f), 'utf8');

test("setup's fingerprint is made from the player's files, never the serialized profile", () => {
  const src = read('src-tauri/src/inputs.rs');
  // signature() fills in the copies; signature_of() makes the fingerprint.
  assert.match(src.match(/^pub fn signature\([^]*?^}/m)[0], /signature_of\(p, g, dir, copies\(p, g\)\?\)/);
  const body = src.match(/^fn signature_of\([^]*?^}/m)[0];
  assert.doesNotMatch(body, /serde_json::to_vec/);
  assert.doesNotMatch(body, /format!\("\{:\?\}"/);
  assert.match(body, /Ok\(format!\("v2:\{\}"/);
  // Older fingerprints are only checked, as two variants, and rewritten as v2.
  assert.match(src, /\[true, false\]\.into_iter\(\)\.any\(\|o\| signature_legacy\(/);
  assert.match(read('src-tauri/src/lib.rs'), /std::thread::spawn\(move \|\| \{\n\s*let st = h\.state::<App>\(\);\n\s*upgrade_fingerprints\(&st, None\);/);
});

test('prepare.version is in the schema, the developer guide and the profile', () => {
  const schema = JSON.parse(read('docs/recomplauncher.schema.json'));
  const find = (o) => (o && typeof o === 'object' ? (o.timeout && o.options && o.version ? o : Object.values(o).map(find).find(Boolean)) : null);
  assert.equal(find(schema).version.type, 'string');
  assert.match(read('docs/DEVELOPERS.md'), /`"version"` \(optional\)/);
  assert.match(read('src-tauri/src/profile.rs'), /skip_serializing_if = "Vec::is_empty"\)\]\n    pub options: Vec<PrepareOption>/);
});

test("an update doesn't open the game's setup by itself", () => {
  assert.match(read('ui/app.js'), /else if \(S\.easy\.has\(id\) && wasInstalled\) toast\(tf\('\{name\} is updated\.'/);
});
