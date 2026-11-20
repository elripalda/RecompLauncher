const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
// Everything the Launcher Support box is made of: the issue it opens, where the setup comes from,
// each feature's state, the Ask box and the card itself.
const block = source.match(/^\/\/ The part of recomplauncher\.json that adds each feature[^]*?^function supportCard\([^]*?^}/m)[0];

// The Launcher Support box on Overview, for a catalog entry and (installed) its own release file.
// `dev` is the developer's account, when they have one: { name, verified }.
function render(entry, local = null, dev = null) {
  const fill = (s, args) => s.replace(/\{(\w+)\}/g, (m, k) => (k in args ? String(args[k]) : m));
  const ctx = vm.createContext({
    S: { library: local ? { game: {} } : {}, easy: new Set(), supLocal: local ? { game: local } : {} },
    invoke: async () => local, builtinBoot: () => ({}), TURNKEY_HOW: [],
    t: (s) => s, N_: (s) => s, tf: fill, curLocale: () => 'en',
    esc: (s) => String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;'),
    devOf: () => dev,
    repoOwner: (a) => (a.repo || '').split('/')[0],
    byDev: (a) => `<span class="bydev">By ${dev ? dev.name : (a.repo || '').split('/')[0]}${dev && dev.verified ? '<svg class="vtick"></svg>' : ''}</span>`,
    isSF: () => false, isLab: () => false, repoHome: (r) => `https://github.com/${r}`, repoIssues: (r) => `https://github.com/${r}/issues`,
    a: { id: 'game', name: 'Example Game', repo: 'studio/game', ...entry },
  });
  vm.runInContext(block, ctx);
  // The block starts the card's own cache empty; the installed file goes in after it.
  ctx.S.supLocal = local ? { game: local } : {};
  return vm.runInContext('supportCard(a)', ctx);
}
const rowOf = (html, name) => html.match(new RegExp(`<b>${name}</b>[^]*?</span></div>`))[0];
const issueOf = (html) => decodeURIComponent(html.match(/body=([^"&]*)/)[1]);
// A release file that sets up everything a project can.
const EVERYTHING = { easy_setup: true, settings: true, prepare: true, verified: true, saves: true };

test('the box no longer says who maintains the setup', () => {
  const html = render({ support: { sdk: 'release', easy_setup: 'dev' } });
  assert.doesNotMatch(html, /maintain/i);
});

test("the installed release's own file makes the setup the project's", () => {
  const html = render({}, { easy_setup: true });
  assert.match(html, /sup-own[^]*From the project/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-p/);
});

test("with the project's own file, a setup feature is never ours", () => {
  const row = rowOf(render({ support: { sdk: 'release', easy_setup: 'dev', settings: 'launcher' } }), 'Game Settings');
  assert.doesNotMatch(row, /mark\.svg/);
  assert.match(row, /sup-no/);
});

test("a setup step the project's own file leaves out is not shown", () => {
  const html = render({}, { easy_setup: true, settings: true });
  assert.doesNotMatch(html, /Full Setup Step/);
  assert.match(html, /2 of 5/);
});

test('without a file of the project, a missing setup step is shown as not yet', () => {
  assert.match(rowOf(render({ support: { sdk: 'none' } }), 'Full Setup Step'), /sup-no/);
});

test("our profile's features show the launcher's logo", () => {
  const html = render({ support: { sdk: 'none', easy_setup: 'launcher' } });
  assert.match(rowOf(html, 'Easy Setup'), /img\/mark\.svg/);
  assert.match(html, /sup-ours[^]*From RecompLauncher/);
});

test('a file only in the repo says so and asks for it in the next release', () => {
  const html = render({ support: { sdk: 'repo', easy_setup: 'dev' } });
  assert.match(html, /In the repo, not in a release yet/);
  assert.match(html, /add it to their next release/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-no/);
});

test("a Forgejo project's repo file counts as the project's", () => {
  const html = render({ launcher_from: 'repo', launcher: { format: 1, game: {}, start: { program: ['game.exe'] } } });
  assert.match(html, /From the project/);
  assert.match(rowOf(html, 'Easy Setup'), /sup-p/);
});

test('everything from the project shows Full Support instead of a count', () => {
  const html = render({}, EVERYTHING, { name: 'Dev', verified: true });
  assert.match(html, /sup-full[^]*Full Support/);
  assert.doesNotMatch(html, /\d of \d/);
});

test('a verified developer with Full Support gets no Ask box', () => {
  assert.doesNotMatch(render({}, EVERYTHING, { name: 'Dev', verified: true }), /sup-ask/);
});

test('an unverified developer with Full Support is asked to verify', () => {
  const html = render({}, EVERYTHING);
  assert.match(html, /Ask to Verify/);
  assert.match(html, /studio ships their own setup file/);
});

test('features we fill in are named in the Ask box', () => {
  const html = render({ copy_ids: ['SLUS-20000'] }, { easy_setup: true, settings: true, prepare: true, saves: true }, { name: 'Dev', verified: true });
  assert.match(rowOf(html, 'Verified Copies'), /img\/mark\.svg/);
  assert.match(html, /RecompLauncher fills in Verified Copies for this game/);
  assert.doesNotMatch(html, /Full Support<\/span>/);
});

test("the verified developer's name gets the tick", () => {
  assert.match(render({}, EVERYTHING, { name: 'Dev', verified: true }), /By Dev<svg class="vtick"/);
});

test('the issue links the AI agent file and the guide', () => {
  const issue = issueOf(render({ support: { sdk: 'none' } }, null, { name: 'Dev', verified: true }));
  assert.match(issue, /recomplauncher-agent\.md/);
  assert.match(issue, /recomplauncher\.com\/sdk/);
});

test("the issue names each missing feature's part of the file", () => {
  const issue = issueOf(render({}, { easy_setup: true, settings: true, prepare: true, saves: true }, { name: 'Dev', verified: true }));
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
  const row = rowOf(render({ launcher: { format: 1, saves: ['{local}/Game/saves'] } }), 'Cloud Saves');
  assert.match(row, /RecompLauncher knows where it saves/);
  assert.match(row, /img\/mark\.svg/);
});

test('cloud saves come from the project when its installed file names the save folder', () => {
  const row = rowOf(render({}, { saves: true }), 'Cloud Saves');
  assert.match(row, /exactly where it saves/);
  assert.match(row, /sup-p/);
});

test("our profile's save folder does not count once the installed release has its own file", () => {
  const row = rowOf(render({ launcher: { format: 1, saves: ['{local}/Game/saves'] } }, { settings: true }), 'Cloud Saves');
  assert.match(row, /isn't known yet/);
});
