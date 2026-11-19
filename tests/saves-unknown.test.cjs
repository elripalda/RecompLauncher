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

// The desktop Saves tab for one game, with the window's helpers kept to what it needs.
function savesTabFor(s) {
  const consts = ['const SAVES_UNKNOWN = ', 'const SAVES_UNKNOWN_HELP = ', 'const SAVES_PICK = ', 'const savesUnknown = '].map((c) => line(account, c)).join('\n');
  const make = new Function('S', `
    const N_ = (x) => x, t = (x) => x, esc = (x) => String(x), tf = (x) => x, tn = (n, a) => a;
    const FROM = {}, CLOUD_ICON = '', SHARE_SAVES = '', SHARE_SAVES_HELP = '';
    const saveShareTitle = () => false, saveShareOn = () => false, loadSaves = () => {};
    const cloudStatus = () => '', copyRow = (id, c) => '<div class="copy">' + c.name + '</div>';
    const window = {};
    ${consts}
    ${fn(account, 'savesTab')}
    return savesTab;`);
  const S = { saves: { g: s }, library: { g: {} }, running: new Set(), settings: { ui: {} } };
  return make(S)({ id: 'g' });
}

const unknownGame = { spots: [], local: [], signedIn: false };
const knownGame = { spots: [{ path: '/saves', template: '{install}/save', from: 'developer', exists: true }], local: [], signedIn: false };

test('a game with no known save folder says why once, in Where it saves', () => {
  const html = savesTabFor(unknownGame);
  const help = 'Nothing is backed up or sent to the cloud until you pick the folder where it keeps its saves.';
  assert.equal(html.split(help).length - 1, 1);
});

test('a game with no known save folder leads with Pick a Folder', () => {
  const html = savesTabFor(unknownGame);
  assert.ok(html.indexOf('data-act="saves-place"') < html.indexOf('On this PC'));
  assert.match(html, /class="btn sm main" data-act="saves-place"/);
});

test('a game with no known save folder offers no backup or save folder to open', () => {
  const html = savesTabFor(unknownGame);
  assert.doesNotMatch(html, /data-act="saves-now"/);
  assert.doesNotMatch(html, /data-act="saves-open"/);
});

test('a game with a known save folder keeps Back up now as the main button', () => {
  const html = savesTabFor(knownGame);
  assert.match(html, /class="btn sm main" data-act="saves-now"/);
  assert.match(html, /data-act="saves-open"/);
  assert.ok(html.indexOf('On this PC') < html.indexOf('Where it saves'));
});

test('Big Picture puts Pick a Folder first and holds back what needs a folder', () => {
  const list = tv.indexOf('body = `<div class="tv-list">');
  const pick = tv.indexOf('data-key="gs:pick"');
  const share = tv.indexOf('data-key="gs:share"', list);
  assert.ok(list >= 0 && list < pick && pick < share, 'the pick is the first item, so it has the focus');
  assert.match(tv, /\$\{unknown \? '' : `<button type="button" class="tv-item" data-f data-key="gs:now"/);
  assert.match(tv, /data-key="gs:sync"[^>]*sv\.busy \|\| unknown/);
});

test('Big Picture only says a copy is kept after every session when it knows where', () => {
  assert.match(tv, /unknown \? t\(SAVES_UNKNOWN\) : t\('After every session the launcher keeps a copy\. Put an older one back with A\.'\)/);
});
