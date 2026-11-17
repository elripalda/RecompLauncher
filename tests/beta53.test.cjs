const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const rust = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('Skip Easy Setup: per game or for every game, and the page treats those games as plain installs', () => {
  const run = new Function('S', 't', `${fn(app, 'applyEasySkip')}; ${fn(app, 'easySkipItem')}; return { applyEasySkip, easySkipItem };`);
  const S = { settings: { ui: {} }, library: {}, easyAll: new Set(['hopper', 'skyracer']) };
  const { applyEasySkip, easySkipItem } = run(S, (x) => x);
  applyEasySkip();
  assert.deepEqual([...S.easy], ['hopper', 'skyracer']);
  assert.equal(easySkipItem('hopper')[0].check, false);
  // Installed, the menu opens the Easy Setup switch on its Manage tab instead.
  S.library.skyracer = {};
  assert.deepEqual(easySkipItem('skyracer'), [{ act: 'easy-row', id: 'skyracer', label: 'Easy Setup…', icon: 'disc' }]);
  delete S.library.skyracer;
  assert.deepEqual(easySkipItem('plain'), [], 'only games Easy Setup can do get the item');
  S.settings.ui.easy_skip = ['hopper'];
  applyEasySkip();
  assert.deepEqual([...S.easy], ['skyracer']);
  assert.equal(easySkipItem('hopper')[0].check, true);
  S.settings.ui.easy_skip_all = true;
  applyEasySkip();
  assert.deepEqual([...S.easy], []);
  assert.deepEqual(easySkipItem('skyracer'), [], 'no per-game item while every game skips it');
  // Saving settings applies it straight away, and the catalog's list is the starting point.
  assert.match(fn(app, 'saveSettings'), /S\.settings = Object\.assign\(\{\}, S\.settings, patch\);\n  applyEasySkip\(\);/);
  assert.match(app, /S\.easyAll = new Set\(await invoke\('easy_setup'\)\);[^\n]*\n  applyEasySkip\(\);/);
});

test('the menu item and the Settings switch', () => {
  const menu = fn(app, 'gameMenu');
  assert.match(menu, /\.\.\.\(!g \? easySkipItem\(id\) : \[\]\)/);
  assert.match(menu, /act: 'manual'[^\n]*\n\s+\.\.\.easySkipItem\(id\),/);
  assert.match(app, /case 'easy-skip': \{[^]*?await setEasySkip\(id, on\);/);
  assert.match(fn(app, 'setEasySkip'), /saveUi\(\{ easy_skip: \[\.\.\.skip\] \}, true\)/);
  assert.match(app, /sw\('easy_skip_all', ui\.easy_skip_all, t\('Skip Easy Setup'\), 'ui-toggle'\)/);
});

test('a skipped game starts as its own setup leaves it, after that setup when it has one', () => {
  assert.match(rust, /let mut skipped = manual\.is_none\(\) && easy_skipped\(&st\.lib\.lock\(\)\.unwrap\(\)\.settings\.ui, id\);/);
  assert.match(rust, /manual = Some\(library::ManualSetup \{ how: "none"\.into\(\), \.\.Default::default\(\) \}\);/);
  assert.match(rust, /if \(manual\.is_none\(\) \|\| rebuild \|\| skipped\) && !prof/);
  // After the "built from your copy" refusal of Set Up By Hand, so skipping isn't refused.
  assert.ok(rust.indexOf('NEED_WAY|') < rust.indexOf('let mut skipped = manual.is_none()'));
  // A game built from the copy into a folder of its own starts the player's build instead, asked
  // for once when there isn't one.
  assert.match(rust, /return Err\(Error::Msg\(format!\("NEED_OWN_BUILD\|\{\}", home_dir\.display\(\)\)\)\);/);
});

const named = fs.readFileSync(path.join(__dirname, '../ui/setup-inputs.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');

test('Use a Build You Already Have: a line under the copy in Easy Setup, a menu item, and the skipped-game prompt', () => {
  const steps = fs.readFileSync(path.join(__dirname, '../ui/setup-steps.js'), 'utf8');
  // Easy Setup: one quiet line, and the folder shows in the copy's box until Set Up uses it.
  assert.match(fn(steps, 'ssAlt'), /if \(su\.own_build\) return `<p class="ss-alt">\$\{esc\(tf\('Already built \{name\} from your copy, outside the launcher\? Skip the build:'/);
  assert.match(fn(steps, 'stepsRun'), /await invoke\('setup_use_build', ns\.own \? \{ id, folder: ns\.own \} : \{ id \}\);/);
  assert.match(fn(named, 'useOwnBuild'), /invoke\('setup_use_build', \{ id, folder: typeof p === 'string' \? p : p\.path \}\)/);
  assert.match(app, /\/\^NEED_OWN_BUILD\\\|\/\.test/);
  assert.match(fn(app, 'gameMenu'), /act: 'ownbuild'/);
  assert.match(tv, /su\.own_build && !su\.prebuilt \? it\(t\('Use a Build You Already Have'\)/);
  // It says where the game's own setup builds it, when that's outside the launcher's folder for
  // the game (build_where): on the last step, beside what Set Up starts.
  assert.match(fn(steps, 'ssSummary'), /su\.build_where && !m\.skipBuild \? `<div><span>\$\{esc\(t\('Builds into'\)\)\}/);
});

test('the launcher building it again goes back to the setup\'s own folder; an update keeps the player\'s', () => {
  assert.equal((rust.match(/if g\.build_dir\.take\(\)\.is_some\(\) \{/g) || []).length, 2);
  assert.match(rust, /build_dir: prev\.as_ref\(\)\.and_then\(\|p\| p\.build_dir\.clone\(\)\),/);
  assert.match(rust, /if let Some\(dir\) = g\.build_dir\.as_ref\(\)\.filter\(\|d\| d\.is_dir\(\)\) \{\n\s+p\.rebase_build\(&g\.path, dir\);/);
});
