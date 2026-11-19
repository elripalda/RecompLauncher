const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const same = (x) => x;

// The rows of the Manage tab's Files and Saves card, for a signed-in player or not.
const fileRows = (inst, signedIn) => {
  const run = new Function('S', 't', 'esc', `${fn(app, 'manageFileRows')}; return manageFileRows;`);
  return run({ account: { signedIn } }, same, same)('g', inst).join('');
};
// The rows of the Updates card, with the game's updates paused or not.
const updateRows = (inst, held) => {
  const run = new Function('S', 't', 'esc', 'isLab', 'isSF', 'fmtB', 'heldIds', `${fn(app, 'manageUpdateRows')}; return manageUpdateRows;`);
  const no = () => false;
  return run({}, same, same, no, no, String, () => new Set(held ? ['g'] : []))('g', inst).join('');
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

test('the project page is one menu row', () => {
  const run = new Function('t', 'tf', 'isSF', 'isFJ', 'isLab', 'fjServer', 'repoHome', `${fn(app, 'repoItem')}; return repoItem;`);
  const no = () => false;
  const repoItem = run(same, same, no, no, no, same, (r) => `https://github.com/${r}`);
  assert.deepEqual(repoItem({ repo: 'dev/hydro' }), [{ act: 'url', url: 'https://github.com/dev/hydro', label: 'On GitHub', icon: 'web' }]);
  assert.deepEqual(repoItem({}), []);
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
  assert.match(html, /On\. After you play, its saves go to your account\./);
  assert.match(html, /aria-checked="true"[^>]*data-act="saves-cloud"/);
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
