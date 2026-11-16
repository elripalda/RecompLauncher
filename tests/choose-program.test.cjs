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
  assert.match(app, /\$\{easyRow\(id\)\}\n\s+\$\{programRow\(id, inst\)\}/);
  const on = easyRowHtml(true, true);
  assert.match(on, /<div class="set-row" id="easy-row"><div class="k"><b>Easy Setup<\/b><span>The launcher sets this game up with your copy\. Off: you set it up yourself\.<\/span>/);
  assert.match(on, /class="switch" role="switch" aria-checked="true"[^>]*data-act="easy-skip" data-id="g"/);
  assert.match(easyRowHtml(true, false), /aria-checked="false"/);
});

test('no Easy Setup switch on a game Easy Setup can\'t do, or while it\'s skipped for every game', () => {
  assert.equal(easyRowHtml(false, false), '');
  assert.equal(easyRowHtml(true, false, { easy_skip_all: true }), '');
});

test('the game\'s menu: Skip Easy Setup ticked before install, Easy Setup… (to the switch) once installed', () => {
  const item = fn(app, 'easySkipItem');
  assert.match(item, /if \(S\.library\[id\]\) return \[\{ act: 'easy-row', id, label: t\('Easy Setup…'\), icon: 'disc' \}\];/);
  assert.match(item, /return \[\{ act: 'easy-skip', id, label: t\('Skip Easy Setup'\), icon: 'disc', check: /);
  assert.match(app, /case 'easy-row': return showEasyRow\(id\);/);
  const show = fn(app, 'showEasyRow');
  assert.match(show, /S\.nextTab = 'installed';\n\s+go\('game', id\);/);
  assert.match(show, /row\.scrollIntoView\(\{ block: 'center'[^\n]*\n\s+row\.classList\.add\('lit'\);/);
});

test('a game Easy Setup can\'t do goes back with Use Default', () => {
  assert.match(row(MINE, false), /data-act="forget-program"[^>]*>Use Default</);
});

test('with Easy Setup skipped for every game, the way back is Use Default', () => {
  assert.match(row(MINE, true, undefined, { easy_skip_all: true }), /data-act="forget-program"[^>]*>Use Default</);
});

test('the Overview shows Self-Managed under the install date, and the menu marks it on', () => {
  assert.match(app, /\$\{inst && k === t\('Installed'\) \? selfManagedPill\(id\) : ''\}/);
  assert.match(fn(app, 'gameMenu'), /act: 'setexe', id, label: selfManaged\(id\) \? t\('Set Game Executable \(On\)…'\) : t\('Set Game Executable…'\)/);
});

test('Where\'s Your Build? offers the build folder or the game\'s executable, each saying what it does', () => {
  const ask = fn(app, 'askWhereBuild');
  assert.match(ask, /t\('Use My Build Folder'\), t\('The launcher starts it and keeps its settings and save backups\.'\)/);
  assert.match(ask, /t\('Set Game Executable'\), t\('Play just starts that file\. You manage everything else\.'\)/);
  assert.match(ask, /data-x="easy">\$\{esc\(t\('Use Easy Setup Instead'\)\)\}/);
  // Big Picture gets the same choices, with the same lines under them.
  assert.match(ask, /if \(inTV\(\)\) \{\n\s+return ask\(title, [^\n]*sub: folder\[1\][^\n]*sub: program\[1\]/);
});

test('picking in Where\'s Your Build? only sets it; the player presses Play', () => {
  assert.match(app, /const v = await askWhereBuild\(\);\n\s+if \(v === 'folder'\) \{\n\s+try \{ if \(!\(await useOwnBuild\(id\)\)\) return; \} catch \(e\) \{ return fail\(e\); \}\n\s+toast\(t\('Build folder set\. Press Play to start it\.'\)\);\n\s+return render\(true\);/);
  assert.match(app, /if \(v === 'program'\) \{\n\s+try \{ if \(!\(await chooseProgram\(id\)\)\) return; \} catch \(e\) \{ return fail\(e\); \}\n\s+toast\(t\('Game executable set\. Press Play to start it\.'\)\);\n\s+return render\(true\);/);
});

test('Use Default lets the pick go in the core', () => {
  assert.match(app, /case 'forget-program':[^]*?invoke\('forget_program', \{ id \}\)/);
  assert.match(rust, /set_exe, forget_program, own_program_games, open_folder,/);
  assert.match(rust, /own_program: player_program\(&st, &id, &g\),/);
  // Only the player's pick is let go: a program the launcher picked stays.
  assert.match(fn(rust.replace(/^fn /gm, 'function '), 'forget_program'), /if g\.exe_rules == u8::MAX \{\n\s+g\.exe = None;\n\s+g\.exe_rules = 0;/);
});

test('Before you play doesn\'t ask a self-managed install for the player\'s copy', () => {
  const rows = fn(app, 'readyRows');
  assert.match(rows, /const selfRun = !!\(su && su\.own_program\);/);
  assert.match(rows, /if \(cs && !selfRun\) rows\.push\(/);
  assert.match(rows, /else if \(a\.needs && !selfRun\) rows\.push\(/);
  assert.match(rows, /if \(su && su\.needs && su\.update && !selfRun\) rows\.push\(/);
});

// Skip Easy Setup is the one switch for who sets a game up.
const rustFn = (name) => fn(rust.replace(/^fn /gm, 'function '), name);

test('Play starts the picked executable only while Easy Setup is off for the game', () => {
  assert.match(rustFn('player_program'), /let easy_on = p\.easy_setup\(\) && !easy_skipped\(&st\.lib\.lock\(\)\.unwrap\(\)\.settings\.ui, id\);\n\s+\(!easy_on\)\.then_some\(exe\)/);
});

test('Set Game Executable asks first on a game Easy Setup does, then turns it off', () => {
  const pick = fn(app, 'chooseProgram');
  assert.match(pick, /const easyOn = S\.easy\.has\(id\);\n\s+if \(easyOn && !\(await askEasyOff\(id\)\)\) return false;\n\s+const p = await pickFile\(/);
  assert.match(pick, /invoke\('set_exe'[^\n]*\n\s+if \(easyOn\) await setEasySkip\(id, true\);/);
  assert.match(fn(app, 'askEasyOff'), /ask\(t\('Turn Off Easy Setup\?'\)[^\n]*t\('Turn Off'\), v: 'yes', main: true \}, \{ label: t\('Not Now'\), v: null \}/);
});

test('turning Easy Setup back on lets go of the picked executable', () => {
  assert.match(fn(app, 'setEasySkip'), /if \(!on && S\.library\[id\]\) S\.library\[id\] = await invoke\('forget_program', \{ id \}\);/);
  // Every way back on goes through it: the Easy Setup switch (and the menu before install), and Where's Your Build?.
  assert.match(app, /case 'easy-skip': \{[^]*?await setEasySkip\(id, on\);/);
  assert.match(app, /if \(v === 'easy'\) \{\n\s+try \{ await setEasySkip\(id, false\); \}/);
  // That link goes on with the Play already starting: a second playGame would be ignored.
  assert.match(fn(app, 'playGame2'), /if \(v === 'easy'\) \{\n[^\n]*\n[^\n]*\n\s+return playGame2\(id, program\);/);
  assert.doesNotMatch(app, /easy-again/);
});

test('executables picked before turn Easy Setup off at start, and keep it off when it comes back on for every game', () => {
  assert.match(fn(app, 'loadEasy'), /applyEasySkip\(\);\n\s+await skipOwnPrograms\(\);/);
  assert.match(fn(app, 'skipOwnPrograms'), /const add = own\.filter\(\(id\) => S\.easy\.has\(id\)\);/);
  assert.match(app, /if \(el\.dataset\.k === 'easy_skip_all' && \(S\.settings\.ui \|\| \{\}\)\.easy_skip_all\) return easyForEveryGame\(\);/);
  assert.match(fn(app, 'easyForEveryGame'), /invoke\('own_program_games'\)[^\n]*skip\.add\(id\);\n\s+return saveUi\(\{ easy_skip_all: false, easy_skip: \[\.\.\.skip\] \}\);/);
});

test('Choose Program is called Set Game Executable everywhere', () => {
  assert.doesNotMatch(app, /t\('Choose Program/);
  assert.doesNotMatch(app, /N_\('Choose Program/);
  assert.match(rust, /Pick it with Set Game Executable in its Manage menu\./);
});
