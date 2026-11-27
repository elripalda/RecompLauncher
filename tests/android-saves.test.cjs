// Cloud saves on Android: games that keep their saves in the phone's shared storage, once the player
// allows All files access. Checks the pieces the phone needs and that the PC keeps working as before.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const manifest = read('src-tauri/gen/android/app/src/main/AndroidManifest.xml');
const plugin = read('src-tauri/gen/android/app/src/main/java/com/recomplauncher/app/ApkPlugin.kt');
const android = read('src-tauri/src/android.rs');
const lib = read('src-tauri/src/lib.rs');
const cloud = read('src-tauri/src/cloud.rs');
const app = read('ui/app.js');
const account = read('ui/account.js');
const phone = read('ui/phone.js');

test('the app asks for All files access, and storage access on older Android', () => {
  assert.match(manifest, /android\.permission\.MANAGE_EXTERNAL_STORAGE/);
  assert.match(manifest, /READ_EXTERNAL_STORAGE" android:maxSdkVersion="32"/);
  assert.match(manifest, /WRITE_EXTERNAL_STORAGE" android:maxSdkVersion="29"/);
  assert.match(plugin, /fun filesAccess\(invoke: Invoke\)/);
  assert.match(plugin, /fun askFilesAccess\(invoke: Invoke\)/);
  assert.match(plugin, /ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION/);
});

test('the commands are registered, and do nothing on a PC', () => {
  for (const c of ['android_files_access', 'android_ask_files_access', 'android_back']) assert.match(lib, new RegExp(`generate_handler!\\[[^\\]]*\\b${c}\\b`), c);
  const back = lib.match(/async fn android_back[^]*?\n}\n/)[0];
  assert.match(back, /#\[cfg\(target_os = "android"\)\]\s*\{[^}]*finish_open_sessions/);
});

test('Play on Android starts a session for a game with save places, finished when the launcher is back', () => {
  const open = android.match(/pub fn open\([^]*?\n}\n/)[0];
  assert.match(open, /spots_of\(st, id, &g\)/);
  assert.match(open, /set_after_play\(&st\.dir, id, true\)/);
  assert.match(app, /invoke\('android_back'\)/);
});

test('a phone uses only its Android save places, and keeps its cloud copies apart', () => {
  const spots = cloud.match(/pub fn spots_of\([^]*?\n}\n/)[0];
  assert.match(spots, /if cfg!\(target_os = "android"\) \{[^}]*catalog_saves\(&entry, true\)/);
  for (const a of ['save-put', 'save-get']) assert.match(cloud, new RegExp(`"${a}"\\), \\("game", &cloud_id\\(`), a);
  assert.match(cloud, /\("a", "saves"\), \("game", &cloud_id\(id\)\)/);
  assert.match(cloud, /"save-delete", json!\(\{"game": cloud_id\(&id\)/);
});

test('without All files access the game still plays, with a word on why', () => {
  assert.match(cloud, /return Ok\(json!\(\{"action": "no-access"\}\)\)/);
  assert.match(account, /r\.action === 'no-access'/);
});

test('the phone has a Saves tab with restore and a way to allow access', () => {
  assert.match(phone, /saves: N_\('Saves'\)/);
  assert.match(phone, /TABS_FIRST = \[[^\]]*'saves'/);
  assert.match(phone, /saves: viewSaves/);
  for (const c of ['android_files_access', 'android_ask_files_access', 'saves_backup', 'saves_info', 'saves_restore']) assert.match(phone, new RegExp(`invoke\\('${c}'`), c);
});

test("a tap in the phone layout doesn't also run the desktop's action of the same name", () => {
  const click = app.match(/document\.addEventListener\('click', \(e\) => \{[^]*?\n\}\);/)[0];
  assert.match(click.split('\n').slice(0, 5).join('\n'), /closest\('#phone'\)\) return;/);
});
