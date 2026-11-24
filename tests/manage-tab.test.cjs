const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const same = (x) => x;

// The rows of the Manage tab's Files and Saves card, for a signed-in player or not.
// The switch reads cloudOn: the game's own setting, or what was picked for every game.
const cloudOnSrc = app.match(/^const cloudOn = .*$/m)[0];
const fileRows = (inst, signedIn, ui = {}) => {
  const run = new Function('S', 't', 'esc', `${cloudOnSrc}; ${fn(app, 'manageFileRows')}; return manageFileRows;`);
  return run({ account: { signedIn }, settings: { ui } }, same, same)('g', inst).join('');
};
// The rows of the Updates card, with the game's updates paused or not (and Settings → Update All
// Games Automatically off).
const updateRows = (inst, held) => {
  const run = new Function('S', 't', 'esc', 'isLab', 'isSF', 'fmtB', 'heldIds', 'autoUpdateAll', 'autoUpdates', `${fn(app, 'manageUpdateRows')}; return manageUpdateRows;`);
  const no = () => false;
  return run({ easy: new Set(), setup: {} }, same, same, no, no, String, () => new Set(held ? ['g'] : []), no, () => !!inst.auto_update)('g', inst).join('');
};
const GAME = { path: 'F:\\Library\\hydro', repo: 'dev/hydro', tag: 'v1.3.0' };

test('the menu leaves out what a tab of the game\'s page has', () => {
  const menu = fn(app, 'gameMenu');
  for (const act of ['saves-game', 'versions', 'looks', 'mods', 'launch-opts', 'setexe', 'manual', 'auto-up', 'build', 'folder', 'backups', 'move', 'shots']) {
    assert.doesNotMatch(menu, new RegExp(`act: '${act}'`), act);
  }
});

test('the menu keeps what no tab has', () => {
  const menu = fn(app, 'gameMenu');
  for (const act of ['pin', 'fav', 'hide', 'customize', 'desktop', 'steam', 'troubleshoot', 'report']) {
    assert.match(menu, new RegExp(`act: '${act}'`), act);
  }
});

test('Uninstall stays in the menu of an installed game, last and in red', () => {
  assert.match(fn(app, 'gameMenu'), /\.\.\.\(g \? \['-', \{ act: 'uninstall', id, label: t\('Uninstall…'\), cls: 'danger', icon: 'bin' \}\] : \[\]\),\n  \];/);
});

// One submenu of the game's menu, from its label to its closing bracket.
const submenu = (label) => fn(app, 'gameMenu').match(new RegExp(`label: t\\('${label}'\\)[^]*?\\n      \\] \\},`))[0];

test('Play is not in the menu: it is the big button', () => {
  assert.doesNotMatch(fn(app, 'gameMenu'), /act: 'play'/);
});

test('Favorites is under Add To', () => {
  assert.match(submenu('Add To'), /act: 'fav'/);
});

test('Add to Start Menu has its own icon, not the desktop one', () => {
  assert.match(submenu('Add To'), /act: 'app-menu'[^\n]*icon: 'apps'/);
});

test('a game that isn\'t installed has no way to report a problem in its menu', () => {
  assert.match(fn(app, 'gameMenu'), /const links = projectItem\(x\);/);
  assert.equal((fn(app, 'gameMenu').match(/devReportItem\(x\)/g) || []).length, 1, 'only inside Troubleshoot, which is for installed games');
});

test('Troubleshoot holds the checker and both ways to report', () => {
  const sub = submenu('Troubleshoot');
  assert.match(sub, /act: 'troubleshoot', id, label: t\('Check What\\'s Wrong…'\)/);
  assert.match(sub, /act: 'report', id, label: t\('Report to RecompLauncher…'\)/);
  assert.match(sub, /\.\.\.devReportItem\(x\)/);
});

test('Set Up Again is Setup Options in the menu, not a button beside Play', () => {
  assert.match(fn(app, 'gameMenu'), /canSetUpAgain\(id\) \? \[\{ act: 'setup', id, label: t\('Setup Options…'\), icon: 'disc', dot: newOpts \}\]/);
  assert.doesNotMatch(fn(app, 'actions'), /again/);
});

test('new setup options put a dot on the "…" button', () => {
  assert.match(fn(app, 'actions'), /class="\$\{sq\}\$\{fresh \? ' has-new' : ''\}" data-act="menu"/);
});

// The menu's rows as HTML, with the helpers they use.
const rows = (items) => new Function('t', 'esc', 'MI', 'CHECK', 'M_CHEV', `${fn(app, 'menuRows')}; return menuRows;`)(same, same, {}, '✓', '›')(items);

test('a menu row with something new has a dot on its right, and says it to screen readers', () => {
  const html = rows([{ act: 'setup', id: 'g', label: 'Setup Options…', dot: '1 new option' }]);
  assert.match(html, /aria-label="Setup Options… · 1 new option">Setup Options…<span class="m-dot" aria-hidden="true"><\/span><\/button>/);
});

test('a menu row with nothing new has no dot', () => {
  assert.doesNotMatch(rows([{ act: 'setup', id: 'g', label: 'Setup Options…', dot: '' }]), /m-dot|aria-label/);
});

test('the setup window marks the Extras step when it has new options', () => {
  const steps = fs.readFileSync(path.join(__dirname, '../ui/setup-steps.js'), 'utf8');
  assert.match(steps, /\$\{s\.id === 'extras' && m\.fresh\.length \? ' new' : ''\}/);
  assert.match(steps, /const fresh = opts\.filter\(\(o\) => o\.new && \(su\.copy \|\| su\.inputs_ready\)\);/);
});

test('Customize is called Edit Name and Artwork', () => {
  assert.match(fn(app, 'gameMenu'), /act: 'customize', id, label: t\('Edit Name and Artwork…'\)/);
  assert.doesNotMatch(fn(app, 'gameMenu'), /Customize…/);
});

test('the side column has no buttons, and Launcher Support is a box on Overview', () => {
  const rail = fn(app, 'gameRail');
  assert.doesNotMatch(rail, /quickActs|btn/);
  // (Beta 68) The store card can sit between them (Placement in RecompLauncher on the site),
  // (Beta 69) and the spot above Ready to Play holds what the site places there.
  assert.match(rail, /S\.tab === 'overview' \? topSpot\(a\) \+ readyCard\(a\) \+ igSlot\(a, 'side'\) \+ supportCard\(a\) : ''/);
});

test('the facts card leaves out who made it (it is under the title)', () => {
  assert.doesNotMatch(fn(app, 'gameRail'), /t\('By'\)/);
});

// A page that scrolls under a 42px top bar, 800px tall, with one open menu 300px tall.
const scrolled = (style, by) => {
  const main = { scrollTop: 0, getBoundingClientRect: () => ({ bottom: 800 }) };
  const bar = { getBoundingClientRect: () => ({ bottom: 42 }) };
  const menu = { style: Object.assign({ clipPath: '' }, style) };
  menu.getBoundingClientRect = () => {
    const top = menu.style.top ? parseFloat(menu.style.top) : 800 - parseFloat(menu.style.bottom) - 300;
    return { top, bottom: top + 300 };
  };
  const run = new Function('$', '$$', `let menuScroll = 0; ${fn(app, 'scrollMenus')}; return scrollMenus;`);
  const scrollMenus = run((s) => (s === '#main' ? main : bar), () => [menu]);
  main.scrollTop = by;
  scrollMenus();
  return menu.style;
};

test('an open menu moves with the page as it scrolls', () => {
  assert.equal(scrolled({ top: '200px' }, 50).top, '150px');
});

test('a menu that opens upward moves with the page too', () => {
  assert.equal(scrolled({ top: '', bottom: '100px' }, 50).bottom, '150px');
});

test('the part of a menu under the top bar is hidden', () => {
  assert.equal(scrolled({ top: '100px' }, 80).clipPath, 'inset(22px 0 0px 0)');
});

test('a menu fully on the page is not clipped', () => {
  assert.equal(scrolled({ top: '400px' }, 50).clipPath, '');
});

test('scrolling the page moves menus instead of closing them', () => {
  const handler = app.match(/\$\('#main'\)\.addEventListener\('scroll', \(\) => \{[^]*?\n\}, \{ passive: true \}\);/)[0];
  assert.match(handler, /^\$\('#main'\)\.addEventListener\('scroll', \(\) => \{\n\s+scrollMenus\(\);/);
  assert.doesNotMatch(handler, /closeMenu/);
});

// The menu's project row for an entry, with the hosts told apart by their repo's prefix.
const projectRow = (x) => {
  const run = new Function('t', 'tf', 'isSF', 'isFJ', 'isLab', 'fjServer', 'repoHome', 'followLink', 'followLabel', 'onX', `${fn(app, 'projectItem')}; return projectItem;`);
  const pre = (p) => (r) => String(r || '').startsWith(p);
  const follow = (a) => a.follow || '';
  const projectItem = run(same, (s, v) => s.replace('{host}', v.host), pre('sourceforge:'), pre('forgejo:'), pre('gitlab:'), () => 'codeberg.org', (r) => `home:${r}`, follow, () => 'Follow It on X', (u) => u.includes('x.com'));
  return projectItem(x);
};

test('the project row is GitHub with its logo', () => {
  assert.deepEqual(projectRow({ repo: 'dev/hydro' }), [{ act: 'url', url: 'home:dev/hydro', label: 'On GitHub', icon: 'github' }]);
});

test('the project row shows GitLab, SourceForge and Forgejo with their own logos', () => {
  assert.equal(projectRow({ repo: 'gitlab:dev/hydro' })[0].icon, 'gitlab');
  assert.equal(projectRow({ repo: 'sourceforge:hydro' })[0].icon, 'sourceforge');
  assert.deepEqual(projectRow({ repo: 'forgejo:codeberg.org/dev/hydro' })[0], { act: 'url', url: 'home:forgejo:codeberg.org/dev/hydro', label: 'On codeberg.org', icon: 'forgejo' });
});

test('a Coming Soon entry with no repo gets Follow It on X instead', () => {
  assert.deepEqual(projectRow({ follow: 'https://x.com/hydro' }), [{ act: 'url', url: 'https://x.com/hydro', label: 'Follow It on X', icon: 'x' }]);
});

test('no repo and no follow link: no project row', () => {
  assert.deepEqual(projectRow({}), []);
});

test('every logo the project row names is in the menu icons', () => {
  const icons = app.match(/^const MI = \{[^]*?^\};/m)[0];
  for (const k of ['github', 'gitlab', 'sourceforge', 'forgejo', 'x', 'apps', 'wrench']) assert.match(icons, new RegExp(`\\n  ${k}: '`), k);
});

test('Manage groups its rows in cards, with Uninstall last', () => {
  const tab = fn(app, 'manageTab');
  assert.match(tab, /card\(t\('Updates'\), [^\n]*card\(t\('Starting the Game'\), [^\n]*card\(t\('Files and Saves'\), [^\n]*\$\{shotsCard\}<div class="card">\$\{leave\}<\/div>/);
});

test('Cloud saves shows in Manage only when signed in', () => {
  assert.doesNotMatch(fileRows(GAME, false), /saves-cloud/);
  assert.match(fileRows(GAME, true), /aria-checked="false"[^>]*data-act="saves-cloud"/);
});

test('Cloud saves switch is on when the game sends its saves', () => {
  const html = fileRows(Object.assign({ cloud: true }, GAME), true);
  assert.match(html, /On\. Saves for this game are synced to the cloud\./);
  assert.match(html, /aria-checked="true"[^>]*data-act="saves-cloud"/);
});

test('Cloud saves switch is on for a game never switched when every game backs up', () => {
  assert.match(fileRows(GAME, true, { cloud_all: true }), /aria-checked="true"[^>]*data-act="saves-cloud"/);
});

test('the Cloud saves switch redraws the Manage tab', () => {
  assert.match(app, /if \(a === 'saves-cloud' && S\.tab === 'installed' && window\.accountAct\) \{ await window\.accountAct\(a, el\); return refreshGame\(\); \}/);
});

test('Pause Updates is a switch that shows whether updates are paused', () => {
  assert.match(updateRows(GAME, true), /aria-checked="true"[^>]*data-act="hold-up"/);
  assert.match(updateRows(GAME, false), /aria-checked="false"[^>]*data-act="hold-up"/);
});

test('a local build has only its project association under Updates', () => {
  const html = updateRows(Object.assign({ local: true }, GAME), false);
  assert.match(html, /data-act="project-association"/);
  assert.doesNotMatch(html, /hold-up|auto-up|data-act="build"/);
});

test('there is no Relink Setup: Change opens the setup with last time\'s files filled in', () => {
  assert.doesNotMatch(fn(app, 'manageStartRows'), /relink/i);
  assert.doesNotMatch(app, /case 'relink'/);
});

test('the setup window has no Use Them Again line', () => {
  const steps = fs.readFileSync(path.join(__dirname, '../ui/setup-steps.js'), 'utf8');
  const inputs = fs.readFileSync(path.join(__dirname, '../ui/setup-inputs.js'), 'utf8');
  assert.doesNotMatch(steps, /relink|Use Them Again/i);
  assert.doesNotMatch(inputs, /relink/i);
});

test('the setup window starts with last time\'s copy in the game\'s box', () => {
  const inputs = fs.readFileSync(path.join(__dirname, '../ui/setup-inputs.js'), 'utf8');
  assert.match(inputs, /if \(su\.kind\) rows\.unshift\(\{ id: 'game', legacy: true, label: su\.needs, role: 'game', path: su\.copy,/);
});
