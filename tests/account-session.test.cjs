const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const code = fs.readFileSync(path.join(__dirname, '../ui/account.js'), 'utf8');

const account = (name) => ({ signedIn: true, user: { username: name } });
const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};

async function fixture() {
  const events = new Map(), notices = [];
  const ctx = vm.createContext({
    S: {}, window: {}, document: {}, $: () => null,
    T: { event: { listen: (name, callback) => events.set(name, callback) } },
    invoke: async () => ({ signedIn: false }), setInterval: () => {},
    t: (s) => s, N_: (s) => s, toast: (s) => notices.push(s),
  });
  vm.runInContext(code, ctx);
  // Rendering/report fetching are unrelated; account commands and event handlers remain real.
  ctx.renderAccountBits = () => {};
  ctx.loadMyReports = () => {};
  await new Promise(setImmediate);
  ctx.S.account = account('A');
  return { ctx, events, notices };
}

test('a refresh delivered after the signed-out event cannot restore the old display', async () => {
  const { ctx, events } = await fixture(), response = deferred();
  ctx.invoke = () => response.promise;
  const pending = ctx.loadAccount(true);
  events.get('account')({ payload: { signedIn: false } });
  response.resolve(account('A'));
  await pending;
  assert.equal(ctx.S.account.signedIn, false);
});

test('an older refresh cannot replace the newer login display', async () => {
  const { ctx } = await fixture(), old = deferred(), recent = deferred();
  let calls = 0;
  ctx.invoke = () => (++calls === 1 ? old : recent).promise;
  const first = ctx.loadAccount(true), second = ctx.loadAccount(true);
  recent.resolve(account('B'));
  await second;
  old.resolve(account('A'));
  await first;
  assert.equal(ctx.S.account.user.username, 'B');
});

test('a delayed sign-out completion displays the replacement account', async () => {
  const { ctx, events, notices } = await fixture(), signout = deferred();
  ctx.invoke = (command) => {
    if (command === 'account_signout') return signout.promise;
    assert.equal(command, 'account_state');
    return Promise.resolve(account('B'));
  };
  const pending = ctx.signOut();
  events.get('account')({ payload: account('B') });
  await new Promise(setImmediate);
  assert.equal(ctx.S.account.user.username, 'B');
  signout.resolve();
  await pending;
  assert.equal(ctx.S.account.signedIn, true);
  assert.equal(ctx.S.account.user.username, 'B');
  assert.equal(notices.length, 0);
});

test('ordinary sign-out still clears the display and reports completion', async () => {
  const { ctx, notices } = await fixture();
  ctx.invoke = async (command) => command === 'account_state' ? { signedIn: false } : undefined;
  await ctx.signOut();
  assert.equal(ctx.S.account.signedIn, false);
  assert.equal(notices.length, 1);
});

test('switching accounts clears cached saves and reports, including accounts without a uid', async () => {
  for (const users of [[{ uid: '1', username: 'Player' }, { uid: '2', username: 'Player' }], [{ username: 'A' }, { username: 'B' }]]) {
    const { ctx } = await fixture();
    ctx.S.account = { signedIn: true, user: users[0] };
    ctx.S.saves = { game: { versions: ['A-save'] } };
    ctx.S.savesAll = { games: ['A-game'] };
    ctx.S.myReports = { list: ['A-report'] };
    ctx.invoke = async () => ({ signedIn: true, user: users[1] });
    await ctx.loadAccount(true);
    assert.equal(Object.keys(ctx.S.saves).length, 0);
    assert.equal(ctx.S.savesAll, null);
    assert.equal(ctx.S.myReports, null);
  }
});

test('refreshing the same uid after a username change keeps its cached data', async () => {
  const { ctx } = await fixture();
  ctx.S.account = { signedIn: true, user: { uid: '1', username: 'Before' } };
  ctx.S.saves = { game: { versions: ['own-save'] } };
  ctx.S.savesAll = { games: ['own-game'] };
  ctx.S.myReports = { list: ['own-report'] };
  ctx.invoke = async () => ({ signedIn: true, user: { uid: '1', username: 'After' } });
  await ctx.loadAccount(true);
  assert.equal(ctx.S.saves.game.versions[0], 'own-save');
  assert.equal(ctx.S.savesAll.games[0], 'own-game');
  assert.equal(ctx.S.myReports.list[0], 'own-report');
});
