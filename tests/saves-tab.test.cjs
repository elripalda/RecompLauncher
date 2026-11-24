const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const account = read('ui/account.js'), tv = read('ui/tv.js'), app = read('ui/app.js');
const savesRs = read('src-tauri/src/saves.rs'), cloudRs = read('src-tauri/src/cloud.rs');

// Everything from `start` to the brace that closes the first one after it, out of `src`.
function block(src, start) {
  const at = src.indexOf(start);
  assert.ok(at >= 0, start);
  let i = src.indexOf('{', at), depth = 0;
  for (; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) break;
  }
  return src.slice(at, i + 1);
}
const line = (src, start) => {
  const at = src.indexOf(start);
  assert.ok(at >= 0, start);
  return src.slice(at, src.indexOf('\n', at));
};

// The Saves tab's pieces out of account.js, with the window's helpers kept to plain English.
function tab(S) {
  return new Function('S', `
    const N_ = (x) => x, t = (x) => x, tf = (x, o) => x.replace(/\\{(\\w+)\\}/g, (m, k) => o[k]), esc = (x) => String(x);
    const tn = (n, one, many) => (n === 1 ? one : many).replace('{n}', n);
    const whenText = (ts) => 'at ' + ts, fmtB = (b) => (b ? b + ' B' : ''), fmtTime = (s) => s + ' s';
    const saveShareTitle = () => false, saveShareOn = () => false;
    ${line(account, 'const FROM = ')}
    ${line(account, 'const SET_ICON = ')}
    ${line(account, 'const spotSet = ')}
    ${line(account, 'const CURRENT_ICON = ')}
    ${line(account, 'const NEXT_ICON = ')}
    ${line(account, 'const SYNCS_ON_PLAY = ')}
    ${line(account, 'const BIN_ICON = ')}
    ${line(account, 'const HISTORY_SHOWN = ')}
    const MARK_ICONS = { pc: '', cloud: '', other: '' };
    ${block(account, 'function saveHistory(')}
    ${block(account, 'function historyMarks(')}
    ${block(account, 'function historyDetails(')}
    ${block(account, 'function historyRow(')}
    ${block(account, 'function historyShown(')}
    ${block(account, 'function historyToggle(')}
    ${block(account, 'function historyCard(')}
    ${block(account, 'function whereCard(')}
    ${block(account, 'function spotRow(')}
    return { saveHistory, historyShown, historyToggle, historyCard, whereCard };`)(S);
}
const fresh = () => ({ savesMore: new Set() });

const local = (when, hash, device = 'Desk PC') => ({ name: `${when}.zip`, when, hash, device, played: 60, files: 2, size: 100 });
const cloud = (made, hash, v, device = 'Desk PC') => ({ v, hash, made, device, played: 60, size: 100 });

test('a save on this PC and the same save in the cloud are one row', () => {
  const rows = tab(fresh()).saveHistory({ local: [local(100, 'a')], cloud: [cloud(100, 'a', 'v1')] });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].name, '100.zip');
  assert.equal(rows[0].v, 'v1');
});

test('the list is newest first, whichever side a save is on', () => {
  const rows = tab(fresh()).saveHistory({ local: [local(100, 'a'), local(300, 'c')], cloud: [cloud(200, 'b', 'v2')] });
  assert.deepEqual(rows.map((r) => r.when), [300, 200, 100]);
});

test('the save in the game now is marked once', () => {
  const rows = tab(fresh()).saveHistory({ local: [local(300, 'a'), local(100, 'a')], cloud: [], current: 'a' });
  assert.deepEqual(rows.map((r) => r.current), [true, false]);
});

test('restoring an older save moves the mark, not the rows', () => {
  const s = { local: [local(300, 'c'), local(200, 'b'), local(100, 'a')], cloud: [] };
  const before = tab(fresh()).saveHistory({ ...s, current: 'c' });
  const after = tab(fresh()).saveHistory({ ...s, current: 'a' });
  assert.deepEqual(after.map((r) => r.when), before.map((r) => r.when));
  assert.equal(after[2].current, true);
});

test('a save another PC made says which one', () => {
  const S = fresh();
  const html = tab(S).historyCard('g', { local: [], cloud: [cloud(100, 'a', 'v1', 'Steam Deck')], here: 'Desk PC', busy: false }, {}, false);
  assert.match(html, /From Steam Deck/);
});

test('a long list shows the newest six and a way to see the rest', () => {
  const S = fresh();
  const rows = Array.from({ length: 9 }, (_, i) => ({ when: 100 - i, current: false }));
  const t = tab(S);
  assert.equal(t.historyShown('g', rows).length, 6);
  assert.equal(t.historyToggle('g', rows, 3).more, true);
});

test('a restored save older than the newest six is still shown', () => {
  const rows = Array.from({ length: 9 }, (_, i) => ({ when: 100 - i, current: i === 7 }));
  assert.equal(tab(fresh()).historyShown('g', rows).length, 8);
});

test('the whole list shown offers to show fewer again', () => {
  const S = fresh();
  S.savesMore.add('g');
  const rows = Array.from({ length: 9 }, (_, i) => ({ when: 100 - i, current: false }));
  assert.equal(tab(S).historyToggle('g', rows, 0).label, 'Show fewer saves');
});

test('a short list has no show more or fewer', () => {
  const rows = [{ when: 1, current: false }];
  assert.equal(tab(fresh()).historyToggle('g', rows, 0), null);
});

const history = (s) => tab(fresh()).historyCard('g', { local: [], cloud: [], busy: false, ...s }, {}, false);

test('saves that stay on this PC can be backed up by hand', () => {
  assert.match(history({ signedIn: true, cloudOn: false }), /data-act="saves-now"/);
});

test('signed out, saves can be backed up by hand', () => {
  assert.match(history({ signedIn: false, cloudOn: true }), /data-act="saves-now"/);
});

test('with cloud saves on, Sync Now does the copy and Back up now is not offered', () => {
  assert.doesNotMatch(history({ signedIn: true, cloudOn: true }), /data-act="saves-now"/);
});

test('Big Picture offers Back up now only without cloud saves too', () => {
  assert.match(tv, /\$\{unknown \|\| \(sv\.signedIn && on\) \? '' : `<button type="button" class="tv-item" data-f data-key="gs:now"/);
});

const devFolder ={ path: '/saves', template: '{install}/save', from: 'developer', exists: true };
const myFolder = { path: '/mine', template: '/mine', from: 'player', exists: true };

test('a folder its developer set can only be opened', () => {
  const html = tab(fresh()).whereCard('g', { spots: [devFolder] });
  assert.match(html, /data-act="saves-open"/);
  assert.doesNotMatch(html, /data-act="saves-place"/);
  assert.doesNotMatch(html, /data-act="saves-place-add"/);
});

test('a folder its developer set says so in green', () => {
  assert.match(tab(fresh()).whereCard('g', { spots: [devFolder] }), /class="svp-set"[^]*Set by its developer/);
});

test('a folder the player picked can be changed, or more added', () => {
  const html = tab(fresh()).whereCard('g', { spots: [myFolder] });
  assert.match(html, /data-act="saves-place"/);
  assert.match(html, /data-act="saves-place-add"/);
});

// ---------- no dead ends ----------
// Every real situation a game's saves can be in, and what gets the player out of it: the act of
// the status card's button, or 'by itself' when the launcher sorts it out without them (the next
// try, the game closing, a sync finishing, the site turning cloud saves back on, the first play).
const steps = new Function(`
  const N_ = (x) => x, SITE = 'https://recomplauncher.com';
  ${block(account, 'function saveNextStep(')}
  ${block(account, 'function problemStep(')}
  return saveNextStep;`)();
const problem = (kind) => ({ state: 'problem', problem: { kind, text: '' } });
const SITUATIONS = [
  ['nobody said where it saves', { state: 'none', known: false }, {}, 'saves-place'],
  ['nobody said where it saves, played already', { state: 'up-to-date', known: false }, {}, 'saves-place'],
  ['not signed in', { state: 'signed-out' }, {}, 'acc-signin'],
  ['cloud saves off', { state: 'off' }, {}, 'saves-cloud'],
  ['both saves changed', { state: 'conflict' }, {}, 'saves-sync'],
  ['both saves changed while the game runs', { state: 'conflict' }, { running: true }, 'by itself'],
  ['up to date', { state: 'up-to-date' }, {}, 'saves-sync'],
  ['never played', { state: 'none' }, {}, 'by itself'],
  ['a newer save going up after the next session', { state: 'waiting' }, {}, 'saves-sync'],
  ['a copy waiting while offline', { state: 'waiting', problem: { kind: 'offline' } }, {}, 'saves-sync'],
  ['an upload running', { state: 'waiting' }, { phase: 'up' }, 'by itself'],
  ['the game just closed, its save being copied', { state: 'waiting' }, { phase: 'after' }, 'by itself'],
  ['the launcher crashed before copying a session (finished at its next start)', { state: 'waiting' }, {}, 'saves-sync'],
  ['an upload that failed', problem('error'), {}, 'saves-sync'],
  ['cloud saves paused by the site', problem('paused'), {}, 'by itself'],
  ['a save too big to upload', problem('too-big-copy'), {}, 'report'],
  ['a full cloud', problem('full'), {}, 'url'],
  ['a set save folder too big to read', problem('too-big-folder'), {}, 'report'],
  ['a picked save folder too big to read', problem('too-big-folder'), { picked: true }, 'saves-place'],
];

for (const [name, st, ctx, want] of SITUATIONS) {
  test(`no dead end: ${name}`, () => {
    const step = steps(st, ctx.phase, !!ctx.running, !!ctx.picked);
    assert.equal(step ? step.act : 'by itself', want);
  });
}

test('a full cloud opens the account page, where every game\'s cloud saves can be deleted', () => {
  assert.equal(steps(problem('full'), undefined, false, false).url, 'https://recomplauncher.com/account');
});

test('every state the core can send has a situation above', () => {
  const words = [...block(savesRs, 'pub fn sync_state(').matchAll(/(?:return|\{)\s*"([a-z-]+)"/g)].map((m) => m[1]);
  const covered = new Set(SITUATIONS.map(([, st]) => st.state));
  for (const w of words) assert.ok(covered.has(w), w);
});

test('every problem the core can record has a situation above', () => {
  const kinds = [...block(cloudRs, 'fn problem_kind(').matchAll(/return "([a-z-]+)"|^\s*"([a-z-]+)"\s*$/gm)].map((m) => m[1] || m[2]);
  const recorded = [...cloudRs.matchAll(/kind: "([a-z-]+)"\.into\(\)/g)].map((m) => m[1]);
  const covered = new Set(SITUATIONS.map(([, st]) => st.problem && st.problem.kind).filter(Boolean));
  for (const k of [...kinds, ...recorded]) assert.ok(covered.has(k), k);
});

test('every button a status can show does something on the desktop', () => {
  for (const [, st, ctx, want] of SITUATIONS) {
    if (want === 'by itself') continue;
    assert.ok(account.includes(`case '${want}'`) || app.includes(`case '${want}'`), want);
  }
});

test('every button a status can show does something in Big Picture, or says to use the desktop', () => {
  const map = line(tv, 'const TV_STEP = ');
  for (const [, , , want] of SITUATIONS) {
    if (want === 'by itself') continue;
    if (want === 'report' || want === 'url') continue;
    const tvAct = map.match(new RegExp(`'${want}': '([a-z-]+)'`));
    assert.ok(tvAct, want);
    assert.ok(tv.includes(`case '${tvAct[1]}'`), tvAct[1]);
  }
  assert.match(tv, /step\.act === 'report' \? ` \$\{t\('Report problems from the desktop view'\)\}\.`/);
  assert.match(tv, /step\.act === 'url' \? ` \$\{t\('Free up space on your account page at recomplauncher\.com\.'\)\}`/);
});

test('any cloud save but the newest can be deleted', () => {
  const rows = tab(fresh()).saveHistory({ local: [], cloud: [cloud(200, 'b', 'v2'), cloud(100, 'a', 'v1')] });
  assert.deepEqual(rows.map((r) => r.deletable), [false, true]);
});

test('a save only on this PC has nothing to delete from the cloud', () => {
  const rows = tab(fresh()).saveHistory({ local: [local(100, 'a')], cloud: [] });
  assert.equal(rows[0].deletable, false);
});

// ---------- a newer save from another PC ----------
// Play puts it in place by itself, so the status and its row say so instead of offering Restore.
const incoming = new Function(`
  const N_ = (x) => x, t = (x) => x, tf = (x, o) => x.replace(/\\{(\\w+)\\}/g, (m, k) => o[k]);
  const sinceText = () => '25 min ago';
  ${block(account, 'function incomingSave(')}
  ${block(account, 'function incomingBadge(')}
  return { incomingSave, incomingBadge };`)();
const incomingSave = incoming.incomingSave;
const deck = cloud(200, 'new', 'v2', 'Steam Deck');
const fromDeck = { local: [local(100, 'old')], cloud: [deck, cloud(100, 'old', 'v1')], current: 'old', incoming: deck, here: 'Desk PC', busy: false };

test('the save Play brings in says it syncs when you press Play, not Restore', () => {
  const html = tab(fresh()).historyCard('g', fromDeck, {}, false);
  const newest = html.slice(0, html.indexOf('at 100'));
  assert.match(newest, /Syncs when you press Play/);
  assert.doesNotMatch(newest, /data-act="saves-restore"/);
});

test('the status above Play names the PC the newer save came from', () => {
  const b = incoming.incomingBadge(deck);
  assert.equal(b.label, 'Newer save from Steam Deck');
  assert.equal(b.note, 'syncs when you press Play');
});

test('the badge above Play and the Saves tab read the same newer save', () => {
  assert.match(block(account, 'function saveBadgeFor('), /incomingSave\(S\.saveNext\[sid\]/);
  assert.match(block(account, 'function saveStatusCard('), /incomingSave\(S\.saveNext\[sid\]/);
  assert.match(block(account, 'async function loadSaves('), /S\.saveNext\[saveIdOf\(id\)\] = S\.saves\[id\]\.incoming/);
});

test('opening a game page asks the cloud only when this PC thinks it is up to date', () => {
  assert.match(block(account, 'async function loadSaveStatus('), /st\.state === 'up-to-date' \|\| st\.state === 'none'\)\) peekSave\(sid\)/);
});

test('older saves keep their Restore', () => {
  const s = { ...fromDeck, local: [local(100, 'old'), local(50, 'older')] };
  const html = tab(fresh()).historyCard('g', s, {}, false);
  assert.match(html.slice(html.indexOf('at 50')), /data-act="saves-restore"/);
});

test('the save in the game now is never the one coming in', () => {
  const rows = tab(fresh()).saveHistory({ ...fromDeck, current: 'new' });
  assert.deepEqual(rows.map((r) => r.incoming), [false, false]);
});

test('the status says a newer save is coming when this PC thinks it is up to date', () => {
  assert.equal(incomingSave(deck, { state: 'up-to-date' }, undefined, false).device, 'Steam Deck');
});

test('a new PC with no saves yet is told the cloud save syncs on Play', () => {
  assert.equal(incomingSave(deck, { state: 'none' }, undefined, false).v, 'v2');
});

test('a conflict still asks to choose a save', () => {
  assert.equal(incomingSave(deck, { state: 'conflict' }, undefined, false), null);
});

test('while the save comes down, the status says that instead', () => {
  assert.equal(incomingSave(deck, { state: 'up-to-date' }, 'down', false), null);
});

test('while the game runs, the status says that instead', () => {
  assert.equal(incomingSave(deck, { state: 'up-to-date' }, undefined, true), null);
});

test('a game nobody said the save folder of still says so', () => {
  assert.equal(incomingSave(deck, { state: 'up-to-date', known: false }, undefined, false), null);
});

test('the newer save can be synced now with Sync Now, on the desktop and in Big Picture', () => {
  assert.match(line(account, 'const INCOMING_STEP = '), /act: 'saves-sync', label: N_\('Sync Now'\)/);
  assert.match(line(tv, 'const TV_STEP = '), /'saves-sync': 'gs-sync'/);
  assert.match(tv, /const step = next \? INCOMING_STEP : saveNextStep\(/);
});

test('Big Picture gives the save coming in no Restore either', () => {
  assert.match(tv, /const act = r\.current \|\| r\.incoming \? '' :/);
});
