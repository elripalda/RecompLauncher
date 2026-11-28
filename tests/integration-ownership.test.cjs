const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
// Everything the Launcher Support box is made of: the issue it opens, where the setup comes from,
// each feature's state, the Ask box and the card itself.
const block = source.match(/^\/\/ The part of recomplauncher\.json that adds each feature[^]*?^function supportCard\([^]*?^}/m)[0];

const INSTALLED = { path: '/games/game', tag: 'v1.0.0', updated_at: 1 };

// The Launcher Support box on Overview, for a catalog entry and the core's answer for it (`core`:
// what game_support gives before install, or support_local once `installed`). `dev` is the
// developer's account, when they have one: { name, verified }.
function render(entry, core = null, dev = null, installed = false) {
  const fill = (s, args) => s.replace(/\{(\w+)\}/g, (m, k) => (k in args ? String(args[k]) : m));
  const ctx = vm.createContext({
    S: { library: installed ? { game: INSTALLED } : {}, sup: !installed && core ? { game: core } : {}, supLocal: {} },
    invoke: async () => core,
    t: (s) => s, N_: (s) => s, tf: fill, curLocale: () => 'en',
    esc: (s) => String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;'),
    devOf: () => dev,
    repoOwner: (a) => (a.repo || '').split('/')[0],
    byDev: (a) => `<span class="bydev">By ${dev ? dev.name : (a.repo || '').split('/')[0]}${dev && dev.verified ? '<svg class="vtick"></svg>' : ''}</span>`,
    isSF: () => false, isLab: () => false, repoHome: (r) => `https://github.com/${r}`, repoIssues: (r) => `https://github.com/${r}/issues`,
    a: { id: 'game', name: 'Example Game', repo: 'studio/game', ...entry },
  });
  vm.runInContext(block, ctx);
  // The block starts the card's own cache empty; the installed release's answer goes in after it.
  ctx.S.supLocal = installed ? { game: { key: vm.runInContext('supLocalKey', ctx)(INSTALLED), v: core } } : {};
  return vm.runInContext('supportCard(a)', ctx);
}
const rowOf = (html, name) => html.match(new RegExp(`<b>${name}</b>[^]*?</span></div>`))[0];
const issueOf = (html) => decodeURIComponent(html.match(/body=([^"&]*)/)[1]);
// The core's answer for a project's own file that sets up everything a project can.
const ALL_P = { from: 'release', easy: true, tag: true, ours: [], easy_setup: 'p', settings: 'p', prepare: 'p', verified: 'p', saves: 'p' };
const FOLDER = { ...ALL_P, from: 'folder' };
// The catalog's labels for a release whose file has every section, as the store counts them.
const ALL_DEV = { sdk: 'release', easy_setup: 'dev', settings: 'dev', prepare: 'dev', verified: 'dev', saves: 'dev' };

test('the box no longer says who maintains the setup', () => {
  const html = render({ support: { sdk: 'release', easy_setup: 'dev' } });
  assert.doesNotMatch(html, /maintain/i);
});

test("the installed release's own file makes the setup the project's", () => {
  const html = render({}, { from: 'folder', easy: true, tag: true, easy_setup: 'p' }, null, true);
  assert.match(html, /sup-own[^]*From the project/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-p/);
});

test('a feature the core says nobody does is not ticked, whatever the catalog\'s labels say', () => {
  const row = rowOf(render({ support: { sdk: 'release', easy_setup: 'dev', settings: 'launcher' } }, { from: 'release', easy: true, easy_setup: 'p', settings: '' }), 'Game Settings');
  assert.doesNotMatch(row, /mark\.svg/);
  assert.match(row, /sup-no/);
});

test('the project\'s own file with everything in it is Full Support', () => {
  const html = render({ support: ALL_DEV }, ALL_P, { name: 'Dev', verified: true });
  assert.match(html, /sup-full[^]*Full Support/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-p/);
});

// Mario Tennis Fever before install: its release ships a file with every section, so the store
// labels them all the project's, but the catalog carries no copy (the launcher can't read the
// file: config.file is one path, not a list). The core has nothing for it.
test('a release file the catalog has no copy of is not Full Support before install', () => {
  const html = render({ support: ALL_DEV }, null, { name: 'Dev', verified: true });
  assert.doesNotMatch(html, /Full Support<\/span>/);
  assert.doesNotMatch(html, /From the project/);
});

test('a release file the catalog has no copy of ticks none of its features before install', () => {
  const html = render({ support: ALL_DEV }, null, { name: 'Dev', verified: true });
  for (const name of ['Easy Setup', 'Game Settings', 'Full Setup Step', 'Verified Copies', 'Cloud Saves']) assert.match(rowOf(html, name), /sup-no/, name);
});

test('a release file the catalog has no copy of is read once installed, and nobody is asked anything', () => {
  // The copy can be missing for a reason other than a broken file, so the developer isn't blamed.
  const html = render({ support: ALL_DEV }, null, { name: 'Dev', verified: true });
  assert.match(html, /In the release, read once it's installed/);
  assert.doesNotMatch(html, /sup-ask/);
});

// Installed: support_local read the release's file and says what's wrong with it.
const PROBLEM = { problem: '"config[0].file" should be a list.' };

test("an installed release file the launcher can't read shows the problem, not Full Support", () => {
  const html = render({ support: ALL_DEV }, PROBLEM, { name: 'Dev', verified: true }, true);
  assert.match(html, /recomplauncher\.json has a problem/);
  assert.match(html, /&quot;config\[0\]\.file&quot; should be a list\./);
  assert.doesNotMatch(html, /Full Support<\/span>/);
});

test("an installed release file the launcher can't read shows no feature", () => {
  const html = render({ support: ALL_DEV }, PROBLEM, { name: 'Dev', verified: true }, true);
  assert.doesNotMatch(html, /sup-row/);
  assert.doesNotMatch(html, /From the project/);
});

test('Tell the Developer opens an issue with the game, its release and the problem', () => {
  const html = render({ name: 'MTF Recomp', support: ALL_DEV }, PROBLEM, { name: 'Dev', verified: true }, true);
  assert.match(html, /Tell the Developer/);
  const issue = issueOf(html);
  assert.match(issue, /recomplauncher\.json` in MTF Recomp v1\.0\.0/);
  assert.match(issue, /The problem: "config\[0\]\.file" should be a list\./);
  assert.match(decodeURIComponent(html.match(/title=([^"&]*)/)[1]), /can't read recomplauncher\.json in v1\.0\.0/);
});

test('the installed release\'s answer counts once the page has read it, not the one from before', () => {
  // game_support said ours before the update; support_local reads the new release's own file.
  const ctx = vm.createContext({ S: { library: { game: INSTALLED }, sup: { game: { from: 'store', easy: true, easy_setup: 'l' } }, supLocal: {} }, invoke: async () => null });
  vm.runInContext(block.match(/^const supLocalKey = .*$/m)[0].replace(/^const /, 'globalThis.') + '\n' + block.match(/^function supOf\([^]*?^}/m)[0], ctx);
  const a = { id: 'game' };
  assert.equal(ctx.supOf(a).from, 'store');
  ctx.S.supLocal = { game: { key: ctx.supLocalKey(INSTALLED), v: FOLDER } };
  assert.equal(ctx.supOf(a).from, 'folder');
});

test('an installed release file the launcher reads, with everything in it, is Full Support', () => {
  const html = render({ support: ALL_DEV }, FOLDER, { name: 'Dev', verified: true }, true);
  assert.doesNotMatch(html, /has a problem/);
  assert.match(html, /sup-full[^]*Full Support/);
});

test('Easy Setup is ticked only when the core says Install sets the copy up', () => {
  const html = render({ support: ALL_DEV }, { ...ALL_P, easy: false, tag: false, easy_setup: '' }, { name: 'Dev', verified: true });
  assert.match(rowOf(html, 'Easy Setup'), /sup-no/);
  assert.doesNotMatch(html, /Full Support<\/span>/);
});

test("the store's Verified Copies label doesn't count when the core finds no known dumps", () => {
  const html = render({ support: ALL_DEV }, { ...ALL_P, verified: '' }, { name: 'Dev', verified: true });
  assert.match(rowOf(html, 'Verified Copies'), /sup-no/);
  assert.doesNotMatch(html, /Full Support<\/span>/);
});

test("a setup step the project's own file leaves out is not shown", () => {
  const html = render({}, { from: 'folder', easy: true, tag: true, easy_setup: 'p', settings: 'p' }, null, true);
  assert.doesNotMatch(html, /Full Setup Step/);
  assert.match(html, /2 of 5/);
});

test('without a file of the project, a missing setup step is shown as not yet', () => {
  assert.match(rowOf(render({ support: { sdk: 'none' } }), 'Full Setup Step'), /sup-no/);
});

test("our profile's features show the launcher's logo", () => {
  const html = render({ support: { sdk: 'none' } }, { from: 'ours', easy: true, tag: true, easy_setup: 'l' });
  assert.match(rowOf(html, 'Easy Setup'), /img\/mark\.svg/);
  assert.match(html, /sup-ours[^]*From RecompLauncher/);
});

test('an older profile of ours shows the same as ours: the launcher\'s logo', () => {
  const html = render({ support: { sdk: 'none' } }, { from: 'store', easy: true, tag: false, easy_setup: 'l', settings: 'l' });
  assert.match(rowOf(html, 'Game Settings'), /img\/mark\.svg/);
  assert.match(html, /sup-ours[^]*From RecompLauncher/);
});

test('a part our profile fills in for the project\'s own file is ours; the rest is the project\'s', () => {
  const html = render({}, { from: 'folder', easy: true, tag: true, ours: ['saves'], easy_setup: 'p', saves: 'l' }, null, true);
  assert.match(html, /sup-own[^]*From the project/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-p/);
  assert.match(rowOf(html, 'Cloud Saves'), /img\/mark\.svg/);
  assert.doesNotMatch(html, /Full Support<\/span>/);
});

test('a file only in the repo says so and asks for it in the next release', () => {
  const html = render({ support: { sdk: 'repo', easy_setup: 'dev' } });
  assert.match(html, /In the repo, not in a release yet/);
  assert.match(html, /add it to their next release/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-no/);
});

test('a catalog file the core doesn\'t credit is never the project\'s', () => {
  // A catalog from before launcher_from: the launcher still uses it, as ours.
  const html = render({ launcher: { format: 1, game: {}, start: { program: ['game.exe'] } } }, { from: 'store', easy: true, tag: false, easy_setup: 'l' });
  assert.doesNotMatch(html, /From the project/);
  assert.match(rowOf(html, 'Easy Setup'), /img\/mark\.svg/);
});

test('everything from the project shows Full Support instead of a count', () => {
  const html = render({}, FOLDER, { name: 'Dev', verified: true }, true);
  assert.match(html, /sup-full[^]*Full Support/);
  assert.doesNotMatch(html, /\d of \d/);
});

test('a verified developer with Full Support gets no Ask box', () => {
  assert.doesNotMatch(render({}, FOLDER, { name: 'Dev', verified: true }, true), /sup-ask/);
});

test('an unverified developer with Full Support is asked to verify', () => {
  const html = render({}, FOLDER, null, true);
  assert.match(html, /Ask to Verify/);
  assert.match(html, /studio ships their own setup file/);
});

// The project's file without known dumps.
const NO_DUMPS = { ...FOLDER, verified: '' };

test('features we fill in are named in the Ask box', () => {
  const html = render({ copy_ids: ['SLUS-20000'] }, NO_DUMPS, { name: 'Dev', verified: true }, true);
  assert.match(rowOf(html, 'Verified Copies'), /img\/mark\.svg/);
  assert.match(html, /RecompLauncher fills in Verified Copies for this game/);
  assert.doesNotMatch(html, /Full Support<\/span>/);
});

test("the verified developer's name gets the tick", () => {
  assert.match(render({}, FOLDER, { name: 'Dev', verified: true }, true), /By Dev<svg class="vtick"/);
});

test('the issue links the AI agent file and the guide', () => {
  const issue = issueOf(render({ support: { sdk: 'none' } }, null, { name: 'Dev', verified: true }));
  assert.match(issue, /recomplauncher-agent\.md/);
  assert.match(issue, /recomplauncher\.com\/sdk/);
});

test("the issue names each missing feature's part of the file", () => {
  const issue = issueOf(render({}, NO_DUMPS, { name: 'Dev', verified: true }, true));
  assert.match(issue, /Verified Copies: `game\.dumps`/);
});

test('the issue asks an unverified developer to sign up', () => {
  assert.match(issueOf(render({ support: { sdk: 'none' } })), /developers\?signup=1/);
});

test('the issue leaves sign-up out for a verified developer', () => {
  assert.doesNotMatch(issueOf(render({ support: { sdk: 'none' } }, null, { name: 'Dev', verified: true })), /signup/);
});

// Cloud Saves: backups need someone to say where the game saves; nothing is guessed.
test('cloud saves are not ticked when nobody says where the game saves', () => {
  const row = rowOf(render({}), 'Cloud Saves');
  assert.match(row, /isn't known yet/);
  assert.match(row, /sup-no/);
});

test('cloud saves come from the launcher when our profile names the save folder', () => {
  const row = rowOf(render({}, { from: 'ours', easy: false, saves: 'l' }), 'Cloud Saves');
  assert.match(row, /RecompLauncher knows where it saves/);
  assert.match(row, /img\/mark\.svg/);
});

test("cloud saves come from the launcher when the catalog names the save folder", () => {
  const row = rowOf(render({ saves: ['{local}/Game/saves'] }), 'Cloud Saves');
  assert.match(row, /img\/mark\.svg/);
});

test('cloud saves come from the project when its installed file names the save folder', () => {
  const row = rowOf(render({}, { from: 'folder', saves: 'p' }, null, true), 'Cloud Saves');
  assert.match(row, /exactly where it saves/);
  assert.match(row, /sup-p/);
});

test('the box works nothing out from the files itself', () => {
  assert.doesNotMatch(block, /fileCovers|builtinBoot|TURNKEY_HOW|a\.launcher\b|a\.boot\b/);
});
