const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// Mario Kart World with v0.1.11 installed. The release list on disk is from before v0.1.12 came
// out, so what the Updates tab was told first only knows v0.1.11; the list fetched in the
// background then brings v0.1.12, whose x86_64 AppImage is a build for this PC.
const repo = 'adriadam10/mkw-recomp';
const id = 'mkw-recomp';
const installed = { tag: 'v0.1.11' };
const newer = [{ tag_name: 'v0.1.12' }, { tag_name: 'v0.1.11' }];
const fromOlderList = { offered: null, checked: 'v0.1.12', builds: ['v0.1.11'] };

// The production loadRels and relState (and what relState uses), with the backend's answers
// given by the test: get_releases brings the newer list, update_info answers with `info`.
function updatesTab(info) {
  const c = vm.createContext({
    S: { rels: {}, updInfo: {}, jobs: {}, running: new Set(), view: 'game', arg: id, tab: 'updates' },
    t: (x) => x,
    esc: (x) => String(x),
    $$: () => [],
    refreshGame: () => {},
    showTab: () => {},
    window: {},
    document: { body: { classList: { contains: () => false } } },
    invoke: async (cmd) => (cmd === 'get_releases' ? { data: newer } : info()),
  });
  for (const name of ['loadRels', 'askUpdInfo', 'relState', 'latestRel']) {
    if (app.includes(`function ${name}(`)) vm.runInContext(fn(name), c);
  }
  // relState and latestRel compare tags with relKey, a const rather than a function; vm keeps a
  // const in its own script, so it goes on the context as a plain global.
  vm.runInContext(app.match(/^const relKey = .*$/m)[0].replace(/^const /, 'globalThis.'), c);
  return c;
}

test('a release that comes in with a newer list is not marked as having no build', async () => {
  const c = updatesTab(() => new Promise(() => {}));
  c.S.updInfo[id] = fromOlderList;
  await c.loadRels(repo);
  assert.doesNotMatch(c.relState(id, installed, newer, newer[0], 0), /No build for your system/);
});

test('an answer worked out from the older list is dropped when it comes back late', async () => {
  let answer;
  const c = updatesTab(() => new Promise((resolve) => { answer = resolve; }));
  c.relState(id, installed, newer, newer[0], 0);
  await c.loadRels(repo);
  answer(fromOlderList);
  await new Promise((resolve) => setImmediate(resolve));
  assert.doesNotMatch(c.relState(id, installed, newer, newer[0], 0), /No build for your system/);
});

// v0.1.11 installed, with v0.1.12 out but not on offer (say, a pre-release) and v0.1.10 before it.
// Every release has a build for this PC and the store has checked them all.
const three = [{ tag_name: 'v0.1.12' }, { tag_name: 'v0.1.11' }, { tag_name: 'v0.1.10' }];
const settled = { offered: null, checked: 'v0.1.12', builds: ['v0.1.12', 'v0.1.11', 'v0.1.10'] };

test('a release older than the installed one offers Downgrade', () => {
  const c = updatesTab(() => new Promise(() => {}));
  c.S.updInfo[id] = settled;
  assert.match(c.relState(id, installed, three, three[2]), />Downgrade</);
});

test('a newer release that is not the update on offer also offers Update', () => {
  const c = updatesTab(() => new Promise(() => {}));
  c.S.updInfo[id] = settled;
  assert.match(c.relState(id, installed, three, three[0]), /class="btn sm" [^>]*>Update</);
});

test('the update on offer is the main Update button', () => {
  const c = updatesTab(() => new Promise(() => {}));
  c.S.updInfo[id] = { ...settled, offered: 'v0.1.12' };
  assert.match(c.relState(id, installed, three, three[0]), /class="btn sm main"[^>]*>Update</);
});

// The game page's Latest version comes from the store; the pill goes on the release it names.
const withPre = [{ tag_name: 'v0.2.0-rc1', prerelease: true }, { tag_name: 'v0.1.12' }, { tag_name: 'v0.1.11' }];

test('the Latest version pill goes on the release the store names', () => {
  const c = updatesTab(() => new Promise(() => {}));
  assert.equal(c.latestRel({ version: '0.1.11' }, withPre), 'v0.1.11');
});

test('with no store version, the pill goes on the newest release that is not a pre-release', () => {
  const c = updatesTab(() => new Promise(() => {}));
  assert.equal(c.latestRel({ version: '' }, withPre), 'v0.1.12');
});
