const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js');
const tv = read('ui/tv.js');
const account = read('ui/account.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const same = (x) => x;

// The Coming Soon part of app.js (SAVE_ICON, savedSoon, soonNoteText, soonNote) with a stand-in
// saved list and account.
function soon({ saved = {}, signedIn = false } = {}) {
  const part = app.match(/^const SAVE_ICON = [^]*?^const soonNote = [^\n]*/m)[0];
  const S = { account: { signedIn } };
  const run = new Function('S', 'isSaved', 't', 'esc', `${part}; return { SAVE_ICON, savedSoon, soonNoteText, soonNote };`);
  return run(S, (id) => !!saved[id], same, same);
}

// The game's main button, with the real actions() and stand-ins for the rest.
function mainButton(a, saved) {
  const run = new Function('S', 'status', 'isSaved', 'esc', 't', 'tf', 'tn', 'canSetUpAgain', 'newPrepOpts', 'wantTag', 'wantCount', 'SAVE_ICON', 'MORE',
    `${fn(app, 'actions')}; return actions;`);
  const icon = (n, on) => (on ? '[full heart]' : '[heart]');
  const actions = run({ library: {}, setup: {} }, () => 'soon', (id) => !!saved[id], same, same, same, same, () => false, () => [], (n) => (n ? `[${n}]` : ''), () => 4704, icon, '');
  return actions(a, 'big', { want: true });
}

test('a Coming Soon game has one Save button, not Notify Me', () => {
  const html = mainButton({ id: 'leonida', soon: true }, {});
  assert.match(html, /data-act="addlib"[^>]*>\[heart\]Save\[4704\]<\/button>/);
  assert.match(html, /class="btn big main"/);
  assert.doesNotMatch(html, /notify/i);
});

test('once saved it says Saved, with a filled heart, and pressing it unsaves', () => {
  const html = mainButton({ id: 'leonida', soon: true }, { leonida: 1 });
  assert.match(html, /class="btn big save-on" data-act="unsave"[^>]*>\[full heart\]Saved\[4704\]<\/button>/);
});

test('a game added by its link stays Coming soon, with no button to press', () => {
  const html = mainButton({ id: 'mine', soon: true, custom: true }, {});
  assert.match(html, /disabled>Coming soon</);
});

test('the line under Save says what it does, and the email only when signed in', () => {
  const a = { id: 'leonida', soon: true };
  assert.equal(soon().soonNoteText(a), 'Save it and we\'ll let you know when it comes out.');
  assert.equal(soon({ saved: { leonida: 1 } }).soonNoteText(a), 'Saved. We\'ll let you know when it comes out.');
  assert.equal(soon({ signedIn: true }).soonNoteText(a), 'Save it and we\'ll let you know here and by email when it comes out.');
  assert.equal(soon({ saved: { leonida: 1 }, signedIn: true }).soonNoteText(a), 'Saved. We\'ll let you know here and by email when it comes out.');
});

test('the line is only on Coming Soon games from the catalog', () => {
  const s = soon();
  assert.match(s.soonNote({ id: 'a', soon: true }), /^<p class="gp-soon-note"/);
  assert.equal(s.soonNote({ id: 'a' }), '');
  assert.equal(s.soonNote({ id: 'a', soon: true, custom: true }), '');
});

test('the heart is filled once it is saved', () => {
  const s = soon();
  assert.match(s.SAVE_ICON(18), /fill="none"/);
  assert.match(s.SAVE_ICON(18, true), /fill="currentColor"/);
});

test('the game page shows the line and keeps it current', () => {
  assert.match(app, /actions\(a, 'big', \{ want: true \}\)\}<\/div>\$\{soonNote\(a\)\}/);
  assert.match(fn(app, 'refreshGame'), /sn\.textContent = soonNoteText\(a\)/);
});

test('saving a Coming Soon game says it will be told about', () => {
  assert.match(fn(app, 'addToLibrary'), /a && a\.soon\) return toast\(tf\('\{name\} is saved\. We\\'ll let you know when it comes out\.'/);
});

test('Notify Me is gone from the launcher', () => {
  for (const src of [app, tv, account]) assert.doesNotMatch(src, /toggleNotify|notifyOn|afterSignIn|notifyForget|'Notify Me'|invoke\('notif/);
  const lib = read('src-tauri/src/lib.rs');
  assert.doesNotMatch(lib, /usage::notif/);
});

test('Big Picture has the same Save, and a line under it', () => {
  assert.match(tv, /isSaved\(id\) \? btn\(key, 'unsave', esc\(t\('Saved'\)\) \+ wants\(\), \{ cls: 'save-on', id, icon: 'heartOn' \}\) : btn\(key, 'addlib', esc\(t\('Save'\)\) \+ wants\(\), \{ cls: 'main', id, icon: 'heart' \}\)/);
  assert.match(tv, /case 'unsave': sfx\('select'\); await removeFromLibrary\(id\)/);
  assert.match(tv, /<p class="tv-soon-note" dir="auto">\$\{esc\(soonNoteText\(a\)\)\}<\/p>/);
});

test('every language has the new words', () => {
  const keys = ['Saved', 'Saved. We\'ll let you know here and by email when it comes out.', 'Saved. We\'ll let you know when it comes out.',
    'Save it and we\'ll let you know here and by email when it comes out.', 'Save it and we\'ll let you know when it comes out.',
    '{name} is saved. We\'ll let you know when it comes out.', 'Recomps on the way. Save one and we\'ll let you know the day it comes out.'];
  for (const f of fs.readdirSync(path.join(__dirname, '../ui/i18n'))) {
    if (!f.endsWith('.json') || f === 'en-GB.json') continue;
    const lang = JSON.parse(read(`ui/i18n/${f}`));
    for (const k of keys) assert.ok(lang[k], `${f}: ${k}`);
    assert.ok(lang['{name} is saved. We\'ll let you know when it comes out.'].includes('{name}'), f);
  }
});
