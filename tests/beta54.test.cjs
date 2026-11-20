const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js'), tv = read('ui/tv.js'), more = read('ui/more.js'), adopt = read('ui/adopt.js');
const rust = read('src-tauri/src/lib.rs'), catalog = read('src-tauri/src/catalog.rs'), own = read('src-tauri/src/own_setup.rs');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('the window opens without waiting long for the scans of the player\'s copies', () => {
  const start = app.slice(app.indexOf('(async function start()'));
  assert.match(start, /const scans = Promise\.all\(\[loadOwned\(\), loadEasy\(true\)\]\);/);
  assert.match(start, /await Promise\.race\(\[scans, new Promise\(\(ok\) => setTimeout\(ok, 400\)\)\]\);/);
  assert.match(start, /if \(!scanned\) scans\.finally\(\(\) => render\(\)\);/);
  assert.doesNotMatch(start, /await Promise\.all\(\[loadOwned\(\), loadEasy\(true\)\]\)/);
  // Each disc image is read once per scan, the list on disk once per change, and the project
  // files once per launch.
  assert.match(rust, /let disc_id = \|p: &Path\| -> Option<String> \{/);
  assert.match(rust, /fn catalog_on_disk\(file: &Path\) -> Value \{/);
  assert.match(catalog, /static ARCHIVED_SEEN:/);
});

test('Big Picture and installs sent from the website ask where the game goes', () => {
  assert.doesNotMatch(tv, /startInstall\(id, S\.settings\.library_dir\)/);
  assert.doesNotMatch(more, /startInstall\(id, S\.settings\.library_dir\)/);
  assert.match(more, /startInstall\(id, S\.library\[id\] \? null : undefined, tag\)/);
  assert.match(fn(app, 'startInstall'), /if \(document\.body\.classList\.contains\('tv-on'\) && window\.tvLocation\) return window\.tvLocation\(id\);\n    return locationModal\(id\);/);
  assert.match(tv, /window\.tvLocation = \(id\) => \{[^]*?saveSettings\(\{ asked_location: true \}\)[^]*?startInstall\(id, dir \|\| S\.settings\.library_dir\)/);
});

test('Pause Updates keeps a game on its version: out of Update, Update All and automatic updates', () => {
  const run = new Function('S', 't', 'tf', `${fn(app, 'heldIds')}; ${fn(app, 'holdUpdates')}; ${fn(app, 'holdItem')}; return { holdUpdates, holdItem };`);
  const S = { settings: { ui: { hold_updates: ['hopper'] } }, updates: { hopper: 'v2', skyracer: 'v5' }, library: { hopper: { name: 'Hopper' }, skyracer: { name: 'SkyRacer' }, mine: { local: true } } };
  const { holdUpdates, holdItem } = run(S, (x) => x, (x) => x);
  holdUpdates();
  assert.deepEqual(S.updates, { skyracer: 'v5' });
  assert.deepEqual(S.held, { hopper: 'v2' });
  assert.equal(holdItem('hopper')[0].check, true);
  assert.equal(holdItem('skyracer')[0].check, false);
  assert.deepEqual(holdItem('mine'), [], 'a local build has no updates to pause');
  // It's applied as updates come in, so automatic updates and Update All never see it.
  const check = fn(app, 'checkUpdates');
  assert.ok(check.indexOf('holdUpdates();') < check.indexOf('const auto = '));
  assert.match(fn(app, 'manageUpdateRows'), /aria-checked="\$\{held\}"[^\n]*data-act="hold-up"/);
  assert.match(app, /case 'hold-up': return toggleHold\(id\);/);
  assert.match(fn(app, 'badNote'), /gp-note held[^]*data-act="hold-up"/);
  assert.match(tv, /window\.toggleHold\(id\)/);
});

test('moving a game where its files already are offers to use them', () => {
  assert.match(rust, /if holds_same_game\(&g, &dest\) \{\n\s+if use_there != Some\(true\) \{\n\s+return Err\(Error::Msg\(format!\("SAME_THERE\|\{\}", dest\.display\(\)\)\)\);/);
  const move = fn(app, 'moveGame');
  assert.match(move, /\/\^SAME_THERE\\\|\(\.\*\)\$\/s/);
  assert.match(move, /invoke\('move_game', \{ id, to, useThere: true \}\)/);
  assert.match(fn(app, 'moveModal'), /await moveGame\(id, to\)/);
  assert.match(adopt, /await moveGame\(f\.id, S\.settings\.library_dir\)/);
});

test('Free Up Space: on the Your Copy card when the game plays without the copy, and to the bin', () => {
  const row = new Function('S', 't', 'tf', 'esc', 'fmtB', `const binName = () => S.os === 'windows' ? t('Recycle Bin') : t('Trash'); ${fn(app, 'spareRow')}; return spareRow;`);
  const S = { os: 'windows' };
  const spareRow = row(S, (x) => x, (x, o) => x.replace(/\{(\w+)\}/g, (_, k) => o[k]), (x) => x, (n) => `${n} B`);
  assert.equal(spareRow('hopper', { spare: null }), '');
  const html = spareRow('hopper', { spare: { path: 'D:/Hopper.iso', size: 9, why: 'built' } });
  assert.match(html, /data-act="free-space" data-id="hopper"/);
  assert.match(html, /Recycle Bin frees 9 B/);
  assert.match(rust, /fn spare_copy\(g: &Installed, p: &profile::Profile, others: &\[Installed\]\) -> Option<SpareCopy>/);
  // Only a build that starts without it, or a disc image unpacked into the game's folder.
  assert.match(rust, /p\.build_done\(&g\.path\) == Some\(true\) && !p\.start_uses_game\(\)/);
  assert.match(rust, /!need\.extract\.is_empty\(\) && copy\.is_file\(\) && p\.already_in_place\(&g\.path\)\.is_some\(\)/);
  assert.match(read('src-tauri/src/files.rs'), /FOF_WANTNUKEWARNING/);
});

test('Move Build moves a build of its own folder and the game starts from there', () => {
  assert.match(fn(app, 'manageStartRows'), /if \(su && su\.build_at\) rows\.push\([^\n]*data-act="move-build"/);
  assert.match(fn(app, 'moveBuild'), /invoke\('move_build', \{ id, to \}\)/);
  assert.match(rust, /x\.build_dir = \(dest != home\)\.then\(\|\| dest\.clone\(\)\);/);
});

test('a setup the game asks for runs, and a setup step that stops says what it said', () => {
  // (Only Play answers it: a shortcut or the page asking doesn't use up the game's request.)
  assert.match(own, /if g\.setup_done && asked_for_setup\(st, id, &g, play\) \{/);
  assert.match(rust, /format!\("Its setup step stopped with code \{code\}: \{said\}"\)/);
  assert.match(read('src-tauri/src/nup.rs'), /fn wrong_folder\(dir: &Path\) -> String/);
  assert.match(rust, /fn say_why_it_stopped\(\)/);
});
