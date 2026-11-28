const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const rust = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('Skip Easy Setup: per game or for every game, and the page treats those games as plain installs', () => {
  const run = new Function('S', 't', `${fn(app, 'applyEasySkip')}; return { applyEasySkip };`);
  const S = { settings: { ui: {} }, library: {}, easyAll: new Set(['hopper', 'skyracer']) };
  const { applyEasySkip } = run(S, (x) => x);
  applyEasySkip();
  assert.deepEqual([...S.easy], ['hopper', 'skyracer']);
  S.settings.ui.easy_skip = ['hopper'];
  applyEasySkip();
  assert.deepEqual([...S.easy], ['skyracer']);
  S.settings.ui.easy_skip_all = true;
  applyEasySkip();
  assert.deepEqual([...S.easy], []);
  // Saving settings applies it straight away, and the catalog's list is the starting point.
  assert.match(fn(app, 'saveSettings'), /S\.settings = Object\.assign\(\{\}, S\.settings, patch\);\n  applyEasySkip\(\);/);
  assert.match(app, /S\.easyAll = new Set\(Object\.keys\(S\.sup\)\.filter\(\(id\) => S\.sup\[id\] && S\.sup\[id\]\.easy\)\);\n  applyEasySkip\(\);/);
});

test('Skip Easy Setup is on the setup window, the Manage tab and in Settings, not in the menu', () => {
  assert.doesNotMatch(fn(app, 'gameMenu'), /easy-skip|Skip Easy Setup/);
  assert.match(fn(app, 'manageStartRows'), /const rows = \[easyRow\(id\), programRow\(id, inst\)\];/);
  assert.match(app, /case 'easy-skip': \{[^]*?await setEasySkip\(id, on\);/);
  assert.match(fn(app, 'setEasySkip'), /saveUi\(\{ easy_skip: \[\.\.\.skip\] \}, true\)/);
  assert.match(app, /sw\('easy_skip_all', !ui\.easy_skip_all, t\('Easy Setup'\), 'easy-all'\)/);
});

test('a skipped game starts as its own setup leaves it, after that setup when it has one', () => {
  assert.match(rust, /let skipped = picked\.is_none\(\) && player_sets_up\(&st, id\);/);
  assert.match(rust, /let own_road = picked\.is_some\(\) \|\| skipped;/);
  assert.match(rust, /if picked\.is_none\(\) && !prof/);
  assert.match(rust, /_ if own_road => \{[^]*?own_plan\(&g, picked\.as_deref\(\)\)\?/);
  // Two roads only: nothing asks a skipped game where its build is, or to set it up another way.
  assert.doesNotMatch(rust, /NEED_OWN_BUILD|NEED_WAY/);
});

const named = fs.readFileSync(path.join(__dirname, '../ui/setup-inputs.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const steps = fs.readFileSync(path.join(__dirname, '../ui/setup-steps.js'), 'utf8');

test('no build of the player\'s own: Easy Setup builds in the folder the manifest names, or Easy Setup is skipped', () => {
  assert.doesNotMatch(steps, /ownpick|keepbuild|data-ss="rebuild"|setup_use_build/);
  assert.doesNotMatch(named, /useOwnBuild|setup_use_build/);
  assert.doesNotMatch(app, /data-act="ownbuild"|askWhereBuild|useOwnBuild/);
  assert.doesNotMatch(rust, /build_dir|prebuilt|fn setup_use_build|fn check_own_build/);
  // Big Picture draws the same steps.
  assert.match(tv, /body = stepsShell\(id, su, TS\.named, a, true\);/);
  // It still says where the game's own setup builds it, when that's outside the launcher's folder.
  assert.match(fn(steps, 'ssSummary'), /su\.build_where \? `<div><span>\$\{esc\(t\('Builds into'\)\)\}/);
});

test('no Set Up By Hand anywhere', () => {
  assert.doesNotMatch(app, /manualModal|set_manual|manual_defaults|Set Up By Hand/);
  assert.doesNotMatch(tv, /manualModal|set_manual|Set Up By Hand/);
  assert.doesNotMatch(rust, /ManualSetup|fn set_manual|fn manual_plan|fill_copy_args/);
});
