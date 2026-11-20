// An update that needs a new build (the setup status's `changed`): Play stays Play with a line under
// it, and opens the update window (update-build.js) instead of Easy Setup, desktop and Big Picture.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8').replace(/\r\n/g, '\n');
const app = read('ui/app.js'), tv = read('ui/tv.js'), upd = read('ui/update-build.js'), css = read('ui/app.css'), html = read('ui/index.html');
const line = (src, name) => src.match(new RegExp(`^const ${name} = [^\\n]*$`, 'm'))[0];
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const tf = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
const esc = (s) => String(s);

// The two checks, on a game's setup status, for an Easy Setup game.
const checks = (status) => new Function('S', `${line(app, 'setupStatus')} ${line(app, 'easyNotReady')} ${line(app, 'updateBuildDue')} return { easyNotReady, updateBuildDue };`)({ easy: new Set(['g']), setup: { g: { status } } });

test('an update that changed the setup step keeps Play and needs the update window', () => {
  const c = checks({ ready: false, reason: 'rebuild', detail: 'kept', changed: true });
  assert.equal(c.updateBuildDue('g'), true);
  assert.equal(c.easyNotReady('g'), false);
});

test('a game brought in by hand (not in the catalog\'s Easy Setup list) gets the update window too', () => {
  const c = new Function('S', `${line(app, 'setupStatus')} ${line(app, 'updateBuildDue')} return { updateBuildDue };`)({ easy: new Set(), setup: { g: { status: { ready: false, reason: 'rebuild', detail: 'kept', changed: true } } } });
  assert.equal(c.updateBuildDue('g'), true);
});

test('its copy rows say the copy isn\'t needed when it builds from what it kept, not to insert it', () => {
  const src = app.slice(app.indexOf('// Easy Setup\'s answer for a game, worked out by the core'), app.indexOf('// Turnkey:'));
  const S = { easy: new Set(['g']), library: { g: { name: 'G' } }, setup: { g: { needs: 'Your copy', copy: null, status: { ready: false, reason: 'rebuild', detail: 'kept', changed: true } } } };
  const c = new Function('S', 't', 'tf', 'byId', `${src} return { copyNotNeeded, copyGoneText };`)(S, (s) => s, tf, () => null);
  assert.equal(c.copyNotNeeded('g'), true);
  assert.equal(c.copyGoneText('g'), 'Your copy isn\'t needed: it\'s built again from what it kept last time.');
  // Built again from the copy: that copy is still asked for.
  S.setup.g.status = { ready: false, reason: 'rebuild', detail: '', changed: true };
  assert.equal(c.copyNotNeeded('g'), false);
  // Before You Play uses it.
  assert.match(fn(app, 'readyRows'), /\|\| copyNotNeeded\(id\)\)\);/);
});

test('the options the update added start as their developer set them', () => {
  assert.match(fn(upd, 'updateBuildModal'), /const updOpts = Object\.fromEntries\(newPrepOpts\(su\)\.map\(\(o\) => \[o\.id, !!o\.on\]\)\);/);
});

test('an unfinished build is still Easy Setup, not the update window', () => {
  const c = checks({ ready: false, reason: 'rebuild', detail: 'kept' });
  assert.equal(c.updateBuildDue('g'), false);
  assert.equal(c.easyNotReady('g'), true);
});

test('a game set up and ready needs neither', () => {
  const c = checks({ ready: true, reason: 'built' });
  assert.equal(c.updateBuildDue('g'), false);
  assert.equal(c.easyNotReady('g'), false);
});

test('the line hangs under Play on the game page only, and takes no room', () => {
  const src = app.match(/^const withBuildLine = [^]*?: html\);$/m)[0];
  const make = (due) => new Function('t', 'esc', 'updateBuildDue', `${src} return withBuildLine;`)((s) => s, esc, () => due);
  const play = '<button>Play</button>';
  assert.equal(make(true)({ id: 'g' }, 'big', play), `<span class="play-wrap">${play}<span class="play-line">Needs a new build after the update</span></span>`);
  assert.equal(make(true)({ id: 'g' }, 'sm', play), play);
  assert.equal(make(false)({ id: 'g' }, 'big', play), play);
  // Both of the installed game's Play buttons on the page carry it.
  const installed = app.match(/^ {4}case 'installed': return setupDue\(a\)[^\n]*$/m)[0];
  assert.equal(installed.match(/withBuildLine\(a, size, b\('main', 'play'/g).length, 2);
  assert.match(css, /\.play-line\{position:absolute;/);
});

test('Play opens the update window, before asking about new options, and after the rebuild error too', () => {
  const play = fn(app, 'playGame');
  assert.ok(play.indexOf('updateBuildModal(id)') < play.indexOf('askNewOpts(id)'));
  assert.match(fn(app, 'askNewOpts'), /if \(updateBuildDue\(id\)\) return false;/);
  assert.match(fn(app, 'playGame2'), /if \(updateBuildDue\(id\)\) return updateBuildModal\(id\);/);
});

test('the window says what it builds from: what it kept, or the copy', () => {
  const lead = new Function('t', 'tf', 'verText', `${fn(upd, 'updLead')} return updLead;`)((s) => s, tf, (v) => v);
  const a = { name: 'Longshot' }, g = { tag: 'v3.3.0' };
  assert.equal(lead(a, { kept: true }, g), 'Longshot v3.3.0 needs a new build of the game. Your copy isn\'t needed: it\'s built again from what it kept last time.');
  assert.equal(lead(a, { kept: false }, g), 'Longshot v3.3.0 needs a new build of the game. It\'s built again from your copy.');
});

test('it runs Easy Setup\'s own setup, from what it kept or the copy, and starts the game when built', () => {
  assert.match(upd, /return suBegin\(su\.kept \? '' : \(su\.copy \|\| ''\), true\);/);
  assert.match(fn(upd, 'updDraw'), /if \(SU\.stage === 'ready'\) \{ closeSetup\(true\); return playGame\(id\); \}/);
  assert.match(fn(app, 'suDraw'), /if \(SU\.upd\) return updDraw\(\);/);
  assert.match(html, /<script src="setup-steps\.js"><\/script>\n<script src="update-build\.js"><\/script>/);
});

test('hiding it lets the build go on in the background, without Easy Setup\'s question', () => {
  const close = fn(app, 'closeSetup');
  const hide = close.indexOf('if (SU.upd && SU.stage === \'run\'');
  assert.ok(hide > 0 && hide < close.indexOf('Setup is still running for {name}'));
  // Play again meanwhile shows that build, not a second one.
  assert.match(fn(upd, 'updateBuildModal'), /const job = S\.setupJobs && S\.setupJobs\[id\];/);
});

test('a build kept from the copy counts as set up for an option the update added', () => {
  const opts = new Function(`${fn(app, 'newPrepOpts')} return newPrepOpts;`)();
  const su = { kept: true, prepare_options: [{ id: 'fsr', new: true }, { id: 'hd', new: false }] };
  assert.deepEqual(opts(su).map((o) => o.id), ['fsr']);
});

test('Big Picture: Play says it in the menu, and builds on its own setup screen', () => {
  assert.match(tv, /run: \(\) => playGame\(id\), sub: updateBuildDue\(id\) \? t\('Needs a new build after the update'\) : ''/);
  assert.match(tv, /window\.tvUpdateBuild = tvUpdateBuild;/);
  assert.match(upd, /if \(document\.body\.classList\.contains\('tv-on'\) && window\.tvUpdateBuild\) return window\.tvUpdateBuild\(id\);/);
  assert.match(tv, /\} else if \(TS\.stage === 'ready' && TS\.upd\) \{/);
});

test('every new text is in every language', () => {
  const texts = ['{name} {version} needs a new build of the game.', 'It\'s built again from your copy.', 'Building the game', 'The Update Didn\'t Finish',
    'Finish the Update', 'Your other choices stay as they were.', 'Your saves and settings stay as they are.', 'Ready to build.', 'Build Now',
    'The game starts when it\'s done.', 'Needs a new build after the update', '{name} keeps building in the background.'];
  const dir = path.join(__dirname, '../ui/i18n');
  // en-GB and fr-CA keep only what they say differently from English and French.
  for (const f of fs.readdirSync(dir).filter((x) => x.endsWith('.json') && x !== 'en-GB.json' && x !== 'fr-CA.json')) {
    const d = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    for (const s of texts) assert.ok(d[s], `${f}: ${s}`);
  }
});
