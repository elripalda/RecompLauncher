const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..');
const read = (f) => fs.readFileSync(path.join(root, f), 'utf8');

test('the Achievements tab asks for the save folder when the unlock file was never found', () => {
  const src = read('ui/achievements.js');
  assert.match(src, /const folder = a\.origin === 'x360' && d\.missing_file/);
  assert.match(src, /t\('Show Me the Folder'\)/);
  assert.match(src, /invoke\('ach_set_folder', \{ id, path: String\(path\), install: g\.path \|\| null \}\)/);
  assert.match(read('ui/games.js'), /'ach_set_disc', 'ach_set_folder'\]/);
});

test('Discord shows the launcher only while its window is open', () => {
  const lib = read('src-tauri/src/lib.rs');
  assert.match(lib, /let hidden = w\.is_minimized\(\)\.unwrap_or\(false\) \|\| !w\.is_visible\(\)\.unwrap_or\(true\);\n\s*away\(w, hidden\);\n[^\n]*\n\s*discord::send\(discord::Msg::Shown\(!hidden\)\);/);
});

test('the last look at the unlock file comes before the session\'s last sync', () => {
  const ach = read('src-tauri/src/ach.rs');
  assert.match(ach, /if let Some\(rx\) = rex_done \{\n\s*let _ = tokio::time::timeout\(Duration::from_secs\(30\), rx\)\.await;\n\s*\}\n\s*checkin\(&a, &sid, json!\(\{\}\)\)\.await;/);
  assert.match(ach, /body\["title_id"\] = json!\(t\);/);
});
