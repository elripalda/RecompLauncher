const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const rust = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('Skip Easy Setup: per game or for every game, and the page treats those games as plain installs', () => {
  const run = new Function('S', 't', `${fn(app, 'applyEasySkip')}; ${fn(app, 'easySkipItem')}; return { applyEasySkip, easySkipItem };`);
  const S = { settings: { ui: {} }, easyAll: new Set(['hopper', 'skyracer']) };
  const { applyEasySkip, easySkipItem } = run(S, (x) => x);
  applyEasySkip();
  assert.deepEqual([...S.easy], ['hopper', 'skyracer']);
  assert.equal(easySkipItem('hopper')[0].check, false);
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
  assert.match(app, /case 'easy-skip': \{[^]*?saveUi\(\{ easy_skip: \[\.\.\.skip\] \}\)/);
  assert.match(app, /sw\('easy_skip_all', ui\.easy_skip_all, t\('Skip Easy Setup'\), 'ui-toggle'\)/);
});

test('a skipped game starts as its own setup leaves it, after that setup when it has one', () => {
  assert.match(rust, /let skipped = manual\.is_none\(\) && easy_skipped\(&st\.lib\.lock\(\)\.unwrap\(\)\.settings\.ui, id\);/);
  assert.match(rust, /manual = Some\(library::ManualSetup \{ how: "none"\.into\(\), \.\.Default::default\(\) \}\);/);
  assert.match(rust, /if \(manual\.is_none\(\) \|\| rebuild \|\| skipped\) && !prof/);
  // After the "built from your copy" refusal of Set Up By Hand, so skipping isn't refused.
  assert.ok(rust.indexOf('NEED_WAY|') < rust.indexOf('let skipped = manual.is_none()'));
});
