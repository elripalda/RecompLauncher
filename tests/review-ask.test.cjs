// "How did it run?" after a session: the player can turn asking off, and a reviewed game is never
// asked about again.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8').replace(/\r\n/g, '\n');
const more = read('more.js'), account = read('account.js'), tv = read('tv.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const line = (src, name) => src.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];

function helpers(S) {
  const c = vm.createContext({ S });
  vm.runInContext(`${line(more, 'reviewAskOn')}\n${line(more, 'reviewedAlready')}\nglobalThis.on = reviewAskOn; globalThis.done = reviewedAlready;`, c);
  return c;
}

test('asking after a session is on until the player turns it off', () => {
  assert.equal(helpers({ settings: { ui: {} } }).on(), true);
  assert.equal(helpers({ settings: {} }).on(), true);
  assert.equal(helpers({ settings: { ui: { review_ask: false } } }).on(), false);
});

test('a game the account has reviewed counts as reviewed', () => {
  const c = helpers({ settings: {}, myReports: { list: [{ app: 'karpur' }] } });
  assert.equal(c.done('karpur'), true);
  assert.equal(c.done('longshot'), false);
  assert.equal(helpers({ settings: {}, myReports: null }).done('karpur'), false);
});

test('after a session nothing is asked when asking is off, the game is reviewed, or it was said never', () => {
  assert.match(fn(more, 'askRun'), /if \(!asked_for && \(!reviewAskOn\(\) \|\| asked\[id\] === 'never' \|\| reviewedAlready\(id\)\)\) return;/);
});

test('a review sent for one version keeps the game from being asked about in the next', () => {
  assert.match(fn(more, 'askRun'), /if \(!r \|\| r\.tag !== g\.tag\) r = \{ tag: g\.tag, asks: 0, waits: 0, sent: sentBefore \};/);
});

test('only the question after a session has Don\'t Ask Me Again', () => {
  assert.match(fn(more, 'reviewDialog'), /const never = auto \? `<label class="rv-never">/);
  assert.match(fn(more, 'askRun'), /if \(!asked_for\) items\.push\(\{ label: t\('Don\\'t Ask Me Again'\)/);
});

test('Settings > Reviews and Big Picture\'s settings both turn asking back on', () => {
  assert.match(fn(account, 'reviewsSection'), /data-act="acc-review-ask"/);
  assert.match(account, /case 'acc-review-ask': \{/);
  assert.match(tv, /data-tvact="reviewask"/);
  assert.match(tv, /case 'reviewask': sfx\('select'\); await window\.setReviewAsk\(!window\.reviewAskOn\(\)\);/);
});
