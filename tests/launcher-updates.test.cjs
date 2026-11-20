const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const more = fs.readFileSync(path.join(__dirname, '../ui/more.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
// Arrays made inside the sandbox aren't this file's arrays, so they're compared as plain data.
const plain = (x) => JSON.parse(JSON.stringify(x));
// A one-line const of app.js, as a global the sandbox can see (vm keeps a const to its own script).
const constLine = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0].replace(/^const /, 'globalThis.');

// Lines as RecompLauncher's release notes write them.
const NOTES = [
  '**This is a beta.** Things may change or break while we\'re in beta.',
  '',
  '### Game pages',
  '- **Cleaner game pages, with rebuilt Updates, Saves, Shaders and Manage tabs.** Buttons stay put. — Improved by @Xarishark ([#323](https://x)).',
  '- **Text boxes fit their text.**',
  '',
  '### Everything else',
  '- **Your play time card counts every PC on your account.** — Fixed by @Xarishark ([#328](https://x)).',
  '- **Cloud saves show their status above Play,** with an easier choice. — Added by @Xarishark ([#319](https://x)).',
].join('\r\n');

function notesContext() {
  const c = vm.createContext({});
  vm.runInContext(fn(app, 'noteItems'), c);
  return c;
}

test('each bullet of the notes is one change, titled by its bold part', () => {
  const items = notesContext().noteItems(NOTES);
  assert.deepEqual(plain(items.map((x) => x.title)), [
    'Cleaner game pages, with rebuilt Updates, Saves, Shaders and Manage tabs',
    'Text boxes fit their text',
    'Your play time card counts every PC on your account',
    'Cloud saves show their status above Play',
  ]);
});

test('a change is Added, Improved or Fixed by its credit, and plain without one', () => {
  const items = notesContext().noteItems(NOTES);
  assert.deepEqual(plain(items.map((x) => x.verb)), ['Improved', '', 'Fixed', 'Added']);
});

test('notes with no bullets have no changes to count', () => {
  assert.equal(notesContext().noteItems('Just a line.\n\nAnother one.').length, 0);
});

// startUpdateDue with the launcher open on the desktop, set up, nothing else on screen.
function dueContext(ui) {
  const c = vm.createContext({
    S: { settings: { setup_done: true, ui }, running: new Set(), flatpak: false },
    document: { body: { classList: { contains: () => false } } },
    $: () => null,
  });
  vm.runInContext(fn(app, 'startUpdateDue') + '\n' + app.match(/^function startsInTV\(.*$/m)[0] + '\n' + constLine('selfAuto'), c);
  return c;
}

test('a version put off with Later waits when updates ask first', () => {
  const c = dueContext({ upd_later: '0.90.0-beta.67' });
  assert.equal(c.startUpdateDue({ version: '0.90.0-beta.67' }), false);
});

test('a version put off with Later still goes in when the launcher updates itself', () => {
  const c = dueContext({ upd_later: '0.90.0-beta.67', self_auto_update: true });
  assert.equal(c.startUpdateDue({ version: '0.90.0-beta.67' }), true);
});

test('a Flatpak never updates itself, whatever the setting says', () => {
  const c = dueContext({ self_auto_update: true });
  c.S.flatpak = true;
  assert.equal(c.selfAuto(), false);
});

// selfUpdatesGroup with the settings page's own row, switch and group markup.
function groupContext(launcherUpd) {
  const c = vm.createContext({
    S: { version: '0.90.0-beta.66', launcherUpd, flatpak: false },
    t: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    verName: (v) => v,
  });
  vm.runInContext(fn(app, 'selfUpdatesGroup'), c);
  const row = (b, s, ctl) => `<row>${b}|${s}|${ctl}</row>`;
  const group = (title, inner) => `<group>${title}:${inner}</group>`;
  const sw = (k, on) => `<sw ${k}=${!!on}>`;
  return (ui = {}) => c.selfUpdatesGroup(group, row, sw, ui);
}

test('the launcher\'s own updates are a group of their own', () => {
  assert.match(groupContext(null)(), /^<group>RecompLauncher Updates:/);
});

test('the group says which version this PC runs', () => {
  assert.match(groupContext(null)(), /You're on 0\.90\.0-beta\.66\./);
});

test('a version that is ready shows with Update Now', () => {
  const html = groupContext({ version: '0.90.0-beta.67' })();
  assert.match(html, /RecompLauncher 0\.90\.0-beta\.67 is ready/);
  assert.match(html, /data-act="self-upd-open"/);
});

test('What\'s New after an update is on until the player turns it off', () => {
  assert.match(groupContext(null)({}), /<sw whats_new_off=true>/);
  assert.match(groupContext(null)({ whats_new_off: true }), /<sw whats_new_off=false>/);
});

test('updating the launcher by itself starts off', () => {
  assert.match(groupContext(null)({}), /<sw self_auto_update=false>/);
});

// The Changelog page with the launcher's releases already loaded.
function changelogContext(count) {
  const list = Array.from({ length: count }, (_, i) => ({ tag_name: `v0.90.0-beta.${66 - i}`, name: `RecompLauncher 0.90 Beta ${66 - i}`, body: '- **A change.**', published_at: '2026-11-20T10:00:00Z' }));
  const c = vm.createContext({
    S: { version: '0.90.0-beta.65', rels: { 'elripalda/RecompLauncher': { list } } },
    t: (s) => s, esc: String, md: (s) => s, date: () => 'Nov 20, 2026', verName: (v) => v,
    CHEV_D: '', loadRels: () => assert.fail('the list is already loaded'),
  });
  vm.runInContext([constLine('relKey'), constLine('SELF_REPO'), constLine('CHANGELOG_STEP'), fn(app, 'changelogPage'), fn(app, 'selfRelDrawer')].join('\n'), c);
  return c;
}
const drawers = (html) => html.match(/<div class="rel[ "]/g) || [];

test('the Changelog shows the ten newest versions at first', () => {
  assert.equal(drawers(changelogContext(30).changelogPage()).length, 10);
});

test('Show More adds the next ten', () => {
  const c = changelogContext(30);
  c.S.clShown = 20;
  assert.equal(drawers(c.changelogPage()).length, 20);
});

test('Show More goes once every version shows', () => {
  assert.doesNotMatch(changelogContext(8).changelogPage(), /data-act="cl-more"/);
});

test('the newest version is marked Latest version and starts open', () => {
  const html = changelogContext(3).changelogPage();
  const first = html.slice(html.indexOf('<div class="rel'), html.indexOf('self@v0.90.0-beta.65'));
  assert.match(first, /class="rel open"/);
  assert.match(first, /Latest version/);
});

test('the version this PC runs is marked Installed', () => {
  const html = changelogContext(3).changelogPage();
  const mine = html.slice(html.indexOf('self@v0.90.0-beta.65'), html.indexOf('self@v0.90.0-beta.64'));
  assert.match(mine, /ai-tag inst/);
});

test('older versions start closed', () => {
  const html = changelogContext(3).changelogPage();
  assert.match(html, /<div class="rel" data-key="self@v0\.90\.0-beta\.64"/);
});

test('the Changelog sits in Settings just above About', () => {
  const sections = app.match(/^const SET_SECTIONS = (.*);$/m)[1];
  assert.match(sections, /\['changelog', N_\('Changelog'\)\], \['about', N_\('About'\)\]\]$/);
});

// The download's progress while the update window is open goes to the window, not a window of its own.
test('the update window gets the download progress', () => {
  let listener;
  const shown = [];
  const win = { id: 'update-window' };
  const c = vm.createContext({
    T: { event: { listen: (_, cb) => { listener = cb; } } },
    $: (sel) => (sel === '.upd-start.busy' ? win : null),
    updWinProgress: (el, p) => shown.push([el.id, p.done]),
    updWinReset: () => assert.fail('nothing failed'),
    modal: () => assert.fail('no second window'),
  });
  vm.runInContext(more.slice(more.indexOf('let updBox = null;'), more.indexOf('// What\'s new:')), c);
  listener({ payload: { done: 5, total: 10 } });
  assert.deepEqual(shown, [['update-window', 5]]);
});

// whatsNew the first time a new version opens, after an update from the previous one.
function whatsNewContext(ui) {
  const saved = [];
  let asked = false;
  const c = vm.createContext({
    S: { version: '0.90.0-beta.66', settings: { ui: { seen_version: '0.90.0-beta.65', self_updates: [{ to: '0.90.0-beta.66' }], ...ui } } },
    saveUi: async (patch) => { saved.push(patch); },
    invoke: async () => { asked = true; return { version: '0.90.0-beta.66', notes: NOTES }; },
    tvOn: () => false, whatsNewCard: () => {}, modal: () => assert.fail('no window after an update'),
  });
  vm.runInContext(fn(more, 'whatsNew'), c);
  return { c, saved, asked: () => asked };
}

test('with What\'s New off, the notes aren\'t fetched after an update', async () => {
  const w = whatsNewContext({ whats_new_off: true });
  await w.c.whatsNew();
  assert.equal(w.asked(), false);
});

test('with What\'s New off, the new version still counts as seen', async () => {
  const w = whatsNewContext({ whats_new_off: true });
  await w.c.whatsNew();
  assert.deepEqual(plain(w.saved.at(-1)), { seen_version: '0.90.0-beta.66' });
});

test('after an update on the desktop, the notes come as a card, not a window', async () => {
  const w = whatsNewContext({});
  let card = null;
  w.c.whatsNewCard = (v, from) => { card = [v.version, from]; };
  await w.c.whatsNew();
  assert.deepEqual(card, ['0.90.0-beta.66', '0.90.0-beta.65']);
});

test('the card goes by itself after ten seconds', () => {
  assert.match(more, /^const WN_MS = 10000;$/m);
});
