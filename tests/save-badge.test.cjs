const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const account = fs.readFileSync(path.join(__dirname, '../ui/account.js'), 'utf8');

// Everything from `start` to the brace that closes the first one after it (a function, or a
// const object), out of account.js.
function block(start) {
  const at = account.indexOf(start);
  assert.ok(at >= 0, start);
  let i = account.indexOf('{', at), depth = 0;
  for (; i < account.length; i++) {
    if (account[i] === '{') depth++;
    if (account[i] === '}' && --depth === 0) break;
  }
  return account.slice(at, i + 1);
}
const line = (start) => {
  const at = account.indexOf(start);
  assert.ok(at >= 0, start);
  return account.slice(at, account.indexOf('\n', at));
};

// The status line's info for a status, with the window's helpers kept to plain English.
const info = new Function(`
  const N_ = (x) => x, t = (x) => x, tf = (x) => x;
  const tn = (n, one, many) => (n === 1 ? one : many).replace('{n}', n);
  const ago = () => 'this week', whenText = (ts) => 'at ' + ts;
  ${block('const PROBLEM_BADGE = ')}
  ${block('const PHASE_BADGE = ')}
  ${block('function sinceText(')}
  ${block('function saveBadgeInfo(')}
  ${block('function problemBadge(')}
  ${block('function waitingBadge(')}
  ${line('const badgeTip = ')}
  return { saveBadgeInfo, sinceText, badgeTip };`)();

const now = () => Math.floor(Date.now() / 1000);
const status = (state, more = {}) => ({ state, syncedAt: 0, waiting: 0, retryIn: null, problem: null, newest: null, ...more });

test('the same save here and in the cloud is green, with how long ago', () => {
  const b = info.saveBadgeInfo(status('up-to-date', { syncedAt: now() - 12 * 60, syncedDevice: 'Desk PC' }), undefined, false);
  assert.equal(b.tone, 'ok');
  assert.equal(b.label, 'Up to date');
  assert.equal(b.note, '12 min ago');
});

test('an upload running shows over what the status says', () => {
  const b = info.saveBadgeInfo(status('waiting', { waiting: 1 }), 'up', false);
  assert.equal(b.label, 'Uploading…');
  assert.equal(b.tone, 'cy');
});

test('a game being played says its save goes up after', () => {
  const b = info.saveBadgeInfo(status('up-to-date', { syncedAt: now() }), undefined, true);
  assert.equal(b.label, 'Syncs after you play');
});

test('with cloud saves off, the copy after a session is not called a sync', () => {
  const b = info.saveBadgeInfo(status('off'), 'after', false);
  assert.equal(b.label, 'Cloud saves off');
});

test('signed out, the copy after a session is not called a sync', () => {
  const b = info.saveBadgeInfo(status('signed-out'), 'after', false);
  assert.equal(b.label, 'Not signed in');
});

test('right after the game closes it still says the save goes up after play, not up to date', () => {
  const b = info.saveBadgeInfo(status('up-to-date', { syncedAt: now() }), 'after', false);
  assert.equal(b.label, 'Syncs after you play');
});

test('a session the launcher has not copied yet is waiting, not up to date', () => {
  const b = info.saveBadgeInfo(status('waiting'), undefined, false);
  assert.equal(b.tone, 'warn');
});

test('a conflict still asks for a choice while the game runs', () => {
  const b = info.saveBadgeInfo(status('conflict'), undefined, true);
  assert.equal(b.label, 'Choose a save');
  assert.equal(b.note, 'press Play to pick');
  assert.equal(b.tone, 'warn');
});

test('a copy queued while offline says so', () => {
  const b = info.saveBadgeInfo(status('waiting', { waiting: 1, problem: { kind: 'offline', text: '' } }), undefined, false);
  assert.equal(b.tone, 'warn');
  assert.equal(b.note, 'offline');
});

test('a copy waiting its next try says when', () => {
  const b = info.saveBadgeInfo(status('waiting', { waiting: 2, retryIn: 230 }), undefined, false);
  assert.equal(b.note, 'next try in 4 min');
});

test('a newer save that goes up after the next session has no note', () => {
  const b = info.saveBadgeInfo(status('waiting'), undefined, false);
  assert.equal(b.label, 'Waiting to upload');
  assert.equal(b.note, '');
});

test('a copy too big to send is red and named', () => {
  const b = info.saveBadgeInfo(status('problem', { problem: { kind: 'too-big-copy', text: 'Over 95 MB.' } }), undefined, false);
  assert.equal(b.tone, 'bad');
  assert.equal(b.label, 'Too big for the cloud');
  assert.equal(b.text, 'Over 95 MB.');
});

test('a problem of a kind it does not know reads as an upload that failed', () => {
  const b = info.saveBadgeInfo(status('problem', { problem: { kind: 'something-new', text: '' } }), undefined, false);
  assert.equal(b.label, 'Can\'t upload');
});

test('cloud saves off is grey', () => {
  const b = info.saveBadgeInfo(status('off'), undefined, false);
  assert.equal(b.tone, 'dim');
  assert.equal(b.label, 'Cloud saves off');
});

test('signed out says so', () => {
  const b = info.saveBadgeInfo(status('signed-out'), undefined, false);
  assert.equal(b.label, 'Not signed in');
});

test('a game never played has nothing to sync', () => {
  const b = info.saveBadgeInfo(status('none'), undefined, false);
  assert.equal(b.label, 'No saves yet');
});

test('the tooltip says what it means, then when it last synced and where', () => {
  const b = info.saveBadgeInfo(status('up-to-date', { syncedAt: 100, syncedDevice: 'Desk PC' }), undefined, false);
  const tip = info.badgeTip(b);
  assert.equal(tip, 'Your save is in the cloud\nThis PC and the cloud have the same save.\nLast synced: at 100\nSaved on: Desk PC');
});

test('a sync over a day ago falls back to the coarse wording', () => {
  assert.equal(info.sinceText(now() - 3 * 86400), 'this week');
});

// Which save the conflict dialog marks Newer.
const newerSave = new Function(`${block('function newerSave(')} return newerSave;`)();

test('this PC saved after the cloud copy: this PC is newer', () => {
  assert.equal(newerSave({ when: 200 }, { made: 100 }), 'local');
});

test('the cloud copy saved after this PC: the cloud one is newer', () => {
  assert.equal(newerSave({ when: 100 }, { made: 200 }), 'cloud');
});

test('saved at the same second: neither is marked', () => {
  assert.equal(newerSave({ when: 100 }, { made: 100 }), '');
});
