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
  for (const act of ['saves-game', 'versions', 'looks', 'mods', 'launch-opts', 'setexe', 'manual', 'auto-up', 'build', 'folder', 'backups', 'move', 'shots', 'uninstall']) {
    assert.doesNotMatch(menu, new RegExp(`act: '${act}'`), act);
  }
});

test('the menu keeps what no tab has', () => {
  const menu = fn(app, 'gameMenu');
  for (const act of ['pin', 'fav', 'hide', 'customize', 'desktop', 'steam', 'troubleshoot', 'report']) {
    assert.match(menu, new RegExp(`act: '${act}'`), act);
  }
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
