// "Checking your copy" while a game's files are read whole: a game on several discs names the disc
// it's on, so the screen doesn't look like it reads Disc 1 four times.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const src = app.match(/^function verifyLine\([^]*?^}/m)[0];
const tf = (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]);
const fmtB = (n) => `${(n / 1e9).toFixed(1)} GB`;
const verifyLine = new Function('tf', 'fmtB', `${src}; return verifyLine;`)(tf, fmtB);

test('a game on several discs names the disc it is reading', () => {
  assert.equal(verifyLine({ part: 'Disc 2', done: 2.1e9, total: 7.1e9 }, ''), 'Disc 2 · 2.1 GB of 7.1 GB');
});

test('a game on one file says how far it got, as before', () => {
  assert.equal(verifyLine({ done: 2.1e9, total: 7.1e9 }, ''), '2.1 GB of 7.1 GB');
});

test('the result replaces the progress once it is in', () => {
  assert.equal(verifyLine({ part: 'Disc 4', done: 7.1e9, total: 7.1e9, text: 'All 4 discs match the verified USA dumps' }, ''), 'All 4 discs match the verified USA dumps');
});

test('before anything is read, the waiting line shows', () => {
  assert.equal(verifyLine(undefined, 'Reading all of it'), 'Reading all of it');
});

test('the bar counts a step finished by its own flag, not by the bytes a read has reached', () => {
  // (a read's events carry `done`, the bytes so far: taken as "finished", the bar jumped to the end of the check)
  assert.match(app, /SU\.steps\[SU\.step\], \{ over: true \}/);
  assert.doesNotMatch(app, /s && s\.done\)/);
  assert.doesNotMatch(fs.readFileSync(path.join(__dirname, '../ui/update-build.js'), 'utf8'), /s && s\.done\)/);
});

test('Easy Setup, Finish the Update and Big Picture all use it', () => {
  for (const f of ['ui/app.js', 'ui/update-build.js', 'ui/tv.js']) {
    assert.match(fs.readFileSync(path.join(__dirname, '..', f), 'utf8'), /verify: verifyLine\(/, f);
  }
});
