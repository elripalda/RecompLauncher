const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const account = read('account.js'), tv = read('tv.js');

// A function declaration out of a file, by name.
function fn(src, name) {
  const start = src.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  let i = src.indexOf('{', start), depth = 0;
  for (; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) break;
  }
  return src.slice(start, i + 1);
}

// One line out of a file: the one that starts with `start`.
const line = (src, start) => {
  const i = src.indexOf(start);
  assert.ok(i >= 0, start);
  return src.slice(i, src.indexOf('\n', i));
};

// The desktop Saves tab for one game. Its cards are stood in for by their names (the cards
// themselves are tested in saves-tab.test.cjs), except the one for an unknown folder.
function savesTabFor(s) {
  const consts = ['const SAVES_UNKNOWN = ', 'const SAVES_UNKNOWN_HELP = ', 'const SAVES_PICK = ', 'const savesUnknown = '].map((c) => line(account, c)).join('\n');
  const make = new Function('S', `
    const N_ = (x) => x, t = (x) => x, esc = (x) => String(x);
    const badgeIcon = () => '', loadSaves = () => {};
    const saveStatusCard = () => '[status]', savesSettingsCard = () => '[settings]', whereCard = () => '[where]';
    const historyCard = (id, s, g, unknown) => (unknown ? '[history, nothing to back up]' : '[history]');
    const window = {};
    ${consts}
    ${fn(account, 'unknownCard')}
    ${fn(account, 'savesTab')}
    return savesTab;`);
  const S = { saves: { g: s }, library: { g: {} }, running: new Set(), settings: { ui: {} } };
  return make(S)({ id: 'g' });
}

const unknownGame = { spots: [], local: [], signedIn: false };
const knownGame = { spots: [{ path: '/saves', template: '{install}/save', from: 'developer', exists: true }], local: [], signedIn: false };

test('a game with no known save folder says why once', () => {
  const html = savesTabFor(unknownGame);
  const help = 'Nothing is backed up or sent to the cloud until you pick the folder where it keeps its saves.';
  assert.equal(html.split(help).length - 1, 1);
});

test('a game with no known save folder leads with Pick a Folder', () => {
  const html = savesTabFor(unknownGame);
  assert.ok(html.indexOf('data-act="saves-place"') < html.indexOf('[history'));
  assert.match(html, /class="btn sm main" data-act="saves-place"/);
});

test('a game with no known save folder is shown in red, since nothing of it uploads', () => {
  assert.match(savesTabFor(unknownGame), /class="card svs sb-bad"/);
});

test('a game with no known save folder has nothing to back up and no folder to show', () => {
  const html = savesTabFor(unknownGame);
  assert.match(html, /\[history, nothing to back up\]/);
  assert.doesNotMatch(html, /\[where\]/);
  assert.doesNotMatch(html, /\[status\]/);
});

test('a game with a known save folder leads with its status, then its saves, then where', () => {
  const html = savesTabFor(knownGame);
  assert.ok(html.indexOf('[status]') < html.indexOf('[history]'));
  assert.ok(html.indexOf('[history]') < html.indexOf('[where]'));
});

test('a known save folder keeps Back up now as the list\'s main button', () => {
  assert.match(fn(account, 'historyCard'), /class="btn sm main" data-act="saves-now"/);
});

test('Big Picture puts Pick a Folder first, in red, and holds back what needs a folder', () => {
  const list = tv.indexOf('body = `<div class="tv-list">');
  const first = tv.indexOf('${unknown ? savesUnknownItem(id) : savesStatusItem(id, sv)}', list);
  const history = tv.indexOf("t('Save History')", list);
  assert.ok(list >= 0 && list < first && first < history, 'the pick is the first item, so it has the focus');
  assert.match(fn(tv, 'savesUnknownItem'), /tv-svs sb-bad[^>]*data-tvact="gs-place"/);
  assert.match(tv, /\$\{unknown \? '' : `<h3 class="tv-lh">\$\{esc\(t\('Where it saves'\)\)\}<\/h3>`\}/);
});

test('Big Picture only says a copy is kept after every session when it knows where', () => {
  assert.match(tv, /unknown \? t\(SAVES_UNKNOWN\) : t\('After every session the launcher keeps a copy\. Restore an older one with A\.'\)/);
});
