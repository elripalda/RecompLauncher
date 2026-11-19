// Linux: every game plays from a plain folder. What the window says when an AppImage is unpacked
// into the Recomp Library, and how the folder it went into is treated.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js'), tv = read('ui/tv.js'), adopt = read('ui/adopt.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const tf = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
const unpackedText = new Function('t', 'tf', `${fn(app, 'unpackedText')}; return unpackedText;`)((s) => s, tf);

test('an AppImage brought in from elsewhere stays where it is', () => {
  assert.equal(unpackedText({ name: 'Hopper', replaced: false, own: false }), 'Hopper is unpacked into your Recomp Library. Your AppImage stays where it is.');
});

test('an AppImage inside the Recomp Library is replaced by the unpacked game', () => {
  assert.equal(unpackedText({ name: 'Hopper', replaced: true, own: false }), 'Hopper is unpacked into your Recomp Library, in place of its AppImage.');
});

test('a game the launcher does not know updates by bringing in its newer AppImage', () => {
  assert.match(unpackedText({ name: 'Mine', replaced: false, own: true }), /Your AppImage stays where it is\. To update it, bring in its newer AppImage the same way\.$/);
});

test('the message comes from one listener, so the desktop and Big Picture say the same', () => {
  assert.match(app, /T\.event\.listen\('unpacked', async \(\{ payload \}\) => \{\n  toast\(unpackedText\(payload \|\| \{\}\), false, null, 12000\);/);
  assert.match(app, /if \(window\.tvRefresh && document\.body\.classList\.contains\('tv-on'\)\) window\.tvRefresh\(\);\n\}\);/);
  assert.doesNotMatch(tv, /listen\('unpacked'/);
});

test('the folder an AppImage was unpacked into is the launcher\'s own, even for a game the player added', () => {
  const ownFolder = new Function('S', `${fn(app, 'ownFolder')}; return ownFolder;`)({ settings: { library_dir: '/home/me/Games/Recomp Library' } });
  assert.equal(ownFolder({ id: 'own-mine', local: true, unpacked: true, path: '/home/me/Games/Recomp Library/Mine' }), true);
  assert.equal(ownFolder({ id: 'own-mine', local: true, path: '/home/me/Games/Recomp Library/Mine' }), false);
});

test('I Already Have It doesn\'t ask where an AppImage plays from: it is always unpacked', () => {
  const haveIt = fn(app, 'haveIt');
  assert.match(haveIt, /const image = S\.os === 'linux' && \/\\\.appimage\$\/i\.test\(String\(r\.exe \|\| ''\)\);/);
  assert.match(haveIt, /const where = image \? 'unpack' : await ask\(/);
  assert.match(tv, /case 'haveit': sfx\('select'\); return window\.haveIt\(id\);/);
});

test('Bring Your Recomps doesn\'t move a game that was unpacked into the library already', () => {
  // Its own folder in the library, unpacked in place, isn't marked the launcher's: it still isn't moved.
  assert.match(adopt, /added\.some\(\(g\) => g\.id === f\.id && !g\.from_appimage\)/);
});

test('the launcher no longer says it unpacks a game the first time it is played', () => {
  assert.doesNotMatch(app, /Unpacking the game the first time it\\'s played/);
  for (const f of fs.readdirSync(path.join(__dirname, '../ui/i18n'))) {
    assert.doesNotMatch(read(`ui/i18n/${f}`), /Unpacking the game the first time it's played/, f);
  }
});
