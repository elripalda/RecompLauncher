const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const rust = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// Game Executable on the game's page, with what game_setup says about the game.
const row = (setup, easy, exe = 'Hookshot\\bin\\hookshot.exe', ui = {}) => {
  const names = ['setupOf', 'selfManaged', 'selfManagedPill', 'exePath', 'programRow'];
  const run = new Function('S', 't', 'esc', `${names.map((n) => fn(app, n)).join(';\n')}; return programRow;`);
  const S = { os: 'windows', settings: { ui }, setup: { g: setup }, easyAll: new Set(easy ? ['g'] : []) };
  return run(S, (x) => x, (x) => x)('g', { exe, path: 'F:\\Library\\hookshot' });
};

test('the path box shows the full path, with Change beside it', () => {
  const html = row(null, false);
  assert.match(html, /<div class="path-box" title="F:\\Library\\hookshot\\Hookshot\\bin\\hookshot\.exe"><bdi>F:\\Library\\hookshot\\Hookshot\\bin\\hookshot\.exe<\/bdi><\/div>/);
  assert.match(html, /data-act="setexe"[^>]*>Change</);
  assert.match(html, /<b>Game Executable<\/b><span>The file Play starts\.<\/span>/);
});

test('a program outside the game\'s folder shows as it is', () => {
  assert.match(row(null, false, 'D:\\Games\\Hookshot\\bin\\hookshot.exe'), /title="D:\\Games\\Hookshot\\bin\\hookshot\.exe"/);
});

test('a game Easy Setup does says what picking its own executable does', () => {
  const html = row({ own_program: null }, true);
  assert.match(html, /Pick the game's own executable to manage it yourself: Play then starts just that file, without Easy Setup\./);
  assert.doesNotMatch(html, /own-pill|forget-program/);
});

const MINE = { own_program: 'F:\\Library\\hookshot\\Hookshot\\bin\\hookshot.exe' };

test('once Play starts the player\'s executable: Self-Managed, and no second button (the Easy Setup switch is the way back)', () => {
  const html = row(MINE, true);
  assert.match(html, /<span class="own-pill" title="You manage this install\. [^"]+">Self-Managed<\/span>/);
  assert.match(html, /You manage this install, and Easy Setup is skipped\./);
  assert.doesNotMatch(html, /forget-program|easy-again|easy-skip/);
});

// The Easy Setup switch on the Manage tab, with what Easy Setup does and Settings say.
const easyRowHtml = (easyAll, easyOn, ui = {}) => {
  const run = new Function('S', 't', 'esc', `${fn(app, 'easyRow')}; return easyRow;`);
  const S = { settings: { ui }, easyAll: new Set(easyAll ? ['g'] : []), easy: new Set(easyOn ? ['g'] : []) };
  return run(S, (x) => x, (x) => x)('g');
};

test('the Easy Setup switch sits above Game Executable and shows whether Easy Setup is on', () => {
  assert.match(app, /const rows = \[easyRow\(id\), programRow\(id, inst\)\];/);
  const on = easyRowHtml(true, true);
  assert.match(on, /<div class="set-row" id="easy-row"><div class="k"><b>Easy Setup<\/b><span>The launcher sets this game up with your copy\. Off: you set it up yourself\.<\/span>/);
  assert.match(on, /class="switch" role="switch" aria-checked="true"[^>]*data-act="easy-skip" data-id="g"/);
  assert.match(easyRowHtml(true, false), /aria-checked="false"/);
});

test('no Easy Setup switch on a game Easy Setup can\'t do, or while it\'s skipped for every game', () => {
  assert.equal(easyRowHtml(false, false), '');
  assert.equal(easyRowHtml(true, false, { easy_skip_all: true }), '');
});

test('turning Easy Setup off says it in the switch\'s words once installed, and how it installs before', () => {
  const sw = app.match(/case 'easy-skip': \{[^]*?\n    \}/)[0];
  assert.match(sw, /if \(!on\) return toast\(tf\('\{name\} uses Easy Setup again\.', \{ name \}\)\);/);
  assert.match(sw, /if \(S\.library\[id\]\) return toast\(tf\('Easy Setup is off for \{name\}\. You set it up yourself\.', \{ name \}\)\);/);
  assert.match(sw, /toast\(tf\('\{name\} skips Easy Setup: it installs as it comes/);
});

test('the setup window offers Skip Easy Setup on its first step, for a game Easy Setup does', () => {
  const steps = fs.readFileSync(path.join(__dirname, '../ui/setup-steps.js'), 'utf8');
  assert.match(steps, /const skippable = m\.at === 0 && S\.easyAll && S\.easyAll\.has\(id\) && !\(S\.settings\.ui \|\| \{\}\)\.easy_skip_all;/);
  assert.match(steps, /data-ss="skip-easy">\$\{esc\(t\('Skip Easy Setup'\)\)\}/);
  assert.match(steps, /if \(act === 'skip-easy'\) return 'skip';/);
});

test('Skip Easy Setup closes the setup window and turns Easy Setup off, on the desktop and in Big Picture', () => {
  const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
  assert.match(app, /if \(r === 'skip'\) \{ await closeSetup\(true\); return skipEasyHere\(id\); \}/);
  assert.match(tv, /if \(r === 'skip'\) \{ closeSheet\(\); return skipEasyHere\(id\); \}/);
  assert.match(fn(app, 'skipEasyHere'), /await setEasySkip\(id, true\);/);
});

test('Skip Easy Setup waits for the window\'s close, which would otherwise put the switch back', () => {
  assert.match(fn(app, 'skipEasyHere'), /await setupClosing;\n\s+try \{ await setEasySkip\(id, true\); \}/);
});

test('a game Easy Setup can\'t do goes back with Use Default', () => {
  assert.match(row(MINE, false), /data-act="forget-program"[^>]*>Use Default</);
});

test('with Easy Setup skipped for every game, the way back is Use Default', () => {
  assert.match(row(MINE, true, undefined, { easy_skip_all: true }), /data-act="forget-program"[^>]*>Use Default</);
});

test('the Overview shows Self-Managed next to the room the game takes', () => {
  assert.match(app, /gsBox\(GS_IC\.disk, t\('On Disk'\), \(size \? esc\(fmtB\(size\)\) : '—'\) \+ selfManagedPill\(id\)\)/);
});

test('Use Default lets the pick go in the core', () => {
  assert.match(app, /case 'forget-program':[^]*?invoke\('forget_program', \{ id \}\)/);
  assert.match(rust, /set_exe, forget_program, set_setup_mode, open_folder,/);
  assert.match(rust, /own_program: player_program\(&st, &id, &g\),/);
  // Only the player's pick is let go: a program the launcher picked stays.
  assert.match(fn(rust.replace(/^fn /gm, 'function '), 'forget_program'), /if g\.exe_rules == u8::MAX \{\n\s+g\.exe = None;\n\s+g\.exe_rules = 0;/);
});

test('Before you play doesn\'t ask a self-managed install for the player\'s copy', () => {
  const rows = fn(app, 'readyRows');
  assert.match(rows, /const selfRun = !!\(su && su\.own_program\);/);
  assert.match(rows, /const askCopy = !selfRun && !building && feedsCopy\(a\);/);
  assert.match(rows, /if \(cs && askCopy\) rows\.push\(/);
  assert.match(rows, /else if \(a\.needs && askCopy\) rows\.push\(/);
  assert.match(rows, /if \(su && su\.needs && askCopy\) rows\.push\(\.\.\.otherFileRows\(id, su\)\);/);
});

test('Before you play doesn\'t ask for the copy while the game builds', () => {
  assert.match(fn(app, 'readyRows'), /const building = status\(a\) === 'settingup' \|\| buildWaits\(id\);/);
});

// The launcher can only hand the copy to the game through Easy Setup or a boot profile's files.
const line = (src, name) => src.match(new RegExp(`^const ${name} = [^\\n]*$`, 'm'))[0];
const feeds = (S, builtin = {}) => new Function('S', 'builtinBoot', `${line(app, 'TURNKEY_HOW')} ${line(app, 'feedsCopy')} return feedsCopy;`)(S, () => builtin);

test('a game that asks for its copy itself isn\'t asked for it on Before you play', () => {
  assert.equal(feeds({ easy: new Set() })({ id: 'g' }), false);
});

test('an Easy Setup game is asked for its copy on Before you play', () => {
  assert.equal(feeds({ easy: new Set(['g']) })({ id: 'g' }), true);
});

test('a game whose boot profile puts its copy in place is asked for it on Before you play', () => {
  assert.equal(feeds({ easy: new Set() }, { g: { files: { how: 'flag' } } })({ id: 'g' }), true);
});

// Skip Easy Setup is the one switch for who sets a game up.
const rustFn = (name) => fn(rust.replace(/^fn /gm, 'function '), name);

test('Play starts the picked executable only while Easy Setup is off for the game', () => {
  assert.match(rustFn('player_program'), /let easy_on = p\.easy_setup\(\) && !player_sets_up\(&st, id\);\r?\n\s+\(!easy_on\)\.then_some\(exe\)/);
});

test('Set Game Executable asks first on a game Easy Setup does, then turns it off', () => {
  const pick = fn(app, 'chooseProgram');
  assert.match(pick, /const easyOn = S\.easy\.has\(id\);\n\s+if \(easyOn && !\(await askEasyOff\(id\)\)\) return false;\n\s+const p = await pickFile\(/);
  assert.match(pick, /invoke\('set_exe'[^\n]*\n\s+if \(easyOn\) await setEasySkip\(id, true\);/);
  assert.match(fn(app, 'askEasyOff'), /ask\(t\('Turn Off Easy Setup\?'\)[^\n]*t\('Turn Off'\), v: 'yes', main: true \}, \{ label: t\('Not Now'\), v: null \}/);
});

test('turning Easy Setup back on keeps the picked executable for switching back', () => {
  // Installed: only the game's mode changes, in the core. Nothing is let go.
  const sw = fn(app, 'setEasySkip');
  assert.match(sw, /S\.library\[rid\] = await invoke\('set_setup_mode', \{ id, mode: on \? 'own' : 'easy' \}\);\r?\n\s+applyEasySkip\(\);/);
  assert.doesNotMatch(sw, /forget_program/);
  // Every way back on goes through it: the Easy Setup switch (and the menu before install).
  assert.match(app, /case 'easy-skip': \{[^]*?await setEasySkip\(id, on\);/);
  assert.doesNotMatch(app, /easy-again/);
});

test('who sets a game up is its own mode: nothing is worked out again at start', () => {
  // Picks made before modes were turned into the game's mode once, by the library converter.
  assert.doesNotMatch(app, /skipOwnPrograms|easyForEveryGame|own_program_games/);
  assert.doesNotMatch(rust, /fn own_program_games/);
  assert.match(fn(app, 'loadEasy'), /applyEasySkip\(\);\r?\n\s+if \(!quiet\) render\(true\);/);
  // Skip Easy Setup for every game sits over each game's mode: turned off, every game is as it was.
  assert.match(app, /case 'ui-toggle':\r?\n(\s+\/\/[^\n]*\n)+\s+return saveUi\(\{ \[el\.dataset\.k\]: !\(S\.settings\.ui \|\| \{\}\)\[el\.dataset\.k\] \}\);/);
});

test('an installed game\'s Easy Setup switch reads its mode', () => {
  const run = new Function('S', `${fn(app, 'applyEasySkip')}; return applyEasySkip;`);
  const S = { settings: { ui: { easy_skip: ['planned'] } }, easyAll: new Set(['hopper', 'skyracer', 'planned']), library: { hopper: { mode: 'own' }, skyracer: {} } };
  run(S)();
  assert.deepEqual([...S.easy], ['skyracer'], 'hopper is the player\'s own; planned was ticked before installing');
  S.settings.ui.easy_skip_all = true;
  run(S)();
  assert.deepEqual([...S.easy], []);
  assert.equal(S.library.skyracer.mode, undefined, 'the switch for every game leaves each game\'s mode as it is');
});

test('Choose Program is called Set Game Executable everywhere', () => {
  assert.doesNotMatch(app, /t\('Choose Program/);
  assert.doesNotMatch(app, /N_\('Choose Program/);
  assert.match(rust, /Pick it with Set Game Executable in its Manage menu\./);
});
