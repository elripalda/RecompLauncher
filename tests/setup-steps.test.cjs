const { test } = require('node:test');
const assert = require('node:assert/strict');
const { stepsContext: context, read } = require('./steps-context.cjs');
const copy = (x) => Object.assign({ needs: 'Your disc', kind: 'iso_or_folder', copy: null, inputs: [], prepare_options: [] }, x);
const disc = (n, p = null) => ({ id: 'disc' + n, label: 'Disc ' + n, role: 'disc', file: true, active: true, path: p });
const dlc = (id, p = null) => ({ id, label: id, role: 'dlc', optional: true, file: true, active: true, path: p });
const vs = (on, extra) => Object.assign({ id: 'vs', label: 'Build tools', required: true, license: { name: 'VS', url: 'https://example.com/vs' }, on }, extra);
const ids = (m) => m.steps.map((s) => s.id).join(',');

test('a game that needs one file and nothing else is one page', () => {
  const c = context();
  const m = c.stepsModel('g', copy(), {});
  assert.equal(ids(m), 'game');
  assert.equal(m.missing.length, 1);
  assert.ok(m.blocked);
});

test('steps with nothing in them are left out: Discs, Add-ons, Extras only when there is something', () => {
  const c = context();
  const su = { needs: 'x', kind: '', inputs: [disc(1), disc(2), dlc('dreams'), { id: 'dlss', label: 'NVIDIA DLSS', role: 'runtime', runtime: true, optional: true, active: true }], prepare_options: [] };
  const m = c.stepsModel('g', su, {});
  assert.equal(ids(m), 'game,addons,extras');
  assert.equal(m.steps[0].name, 'Discs');
  assert.equal(m.missing.length, 2, 'the DLC and DLSS never hold setup back');
});

test('a copy found in the folders fills its box, tagged, and Set Up can start', () => {
  const c = context({ g: { path: '/roms/Game.iso' } });
  const m = c.stepsModel('g', copy(), {});
  assert.equal(m.missing.length, 0);
  const html = c.stepsView('g', copy(), {}, { name: 'Recomp', game: 'Game' });
  assert.match(html, /<span class="ss-src">Found in your folders<\/span>/);
  assert.match(html, /Game\.iso/);
});

test('the copy box keeps the same buttons empty or filled', () => {
  const c = context();
  const btns = (html) => html.match(/data-ss="(file|folder)"/g).join();
  const empty = c.stepsView('g', copy(), {}, { name: 'R', game: 'G' });
  const filled = c.stepsView('g', copy(), { copy: '/roms/G.iso' }, { name: 'R', game: 'G' });
  assert.equal(btns(empty), btns(filled));
  assert.doesNotMatch(filled, /data-ss="clear"/, 'choosing again is how the copy changes');
});

test('a required extra that is off holds Set Up, and marks the Extras step', () => {
  const c = context();
  const su = copy({ copy: '/roms/G.iso', prepare_options: [vs(false)] });
  const m = c.stepsModel('g', su, { at: 1 });
  assert.ok(m.reqOff && m.blocked);
  const html = c.stepsView('g', su, { at: 1 }, { name: 'R' });
  assert.match(html, /class="s now req"/);
  assert.match(html, /<span class="su-req-tag">Required<\/span>/);
  assert.match(html, /Turn on the option marked Required to continue/);
  assert.match(html, /data-ss="run" disabled/);
  su.prepare_options[0].on = true;
  assert.equal(c.stepsModel('g', su, { at: 1 }).blocked, false);
});

test('with a build already there, build options do nothing and hold nothing back', () => {
  const c = context();
  const su = copy({ prebuilt: '/install/Game', prepare_options: [vs(false)] });
  const m = c.stepsModel('g', su, {});
  assert.ok(m.skipBuild);
  assert.equal(m.missing.length, 0, 'the build stands in for the copy');
  assert.equal(m.reqOff, false);
  assert.match(c.stepsView('g', su, { at: 1 }, { name: 'R' }), /<label class="su-opt off">/);
  assert.equal(c.stepsModel('g', su, { rebuild: true }).skipBuild, false, 'Build It Again needs the copy again');
});

test('the player\'s own build shows in the copy\'s box; one that failed holds Set Up', () => {
  const c = context();
  const su = copy({ own_build: '/install/Game' });
  const view = (ns) => c.stepsView('g', su, ns, { name: 'Recomp', game: 'Game' });
  assert.match(view({}), /Already built Recomp from your copy, outside the launcher\? Skip the build: <button type="button" data-ss="ownpick">Choose That Folder<\/button>/);
  assert.match(view({ own: '/games/Recomp' }), /<div class="ss-box on">[^]*Your own build[^]*data-ss="owncopy"/);
  const bad = { own: '/games/x', ownErr: 'That folder is empty.' };
  assert.ok(c.stepsModel('g', su, bad).blocked);
  assert.match(view(bad), /<div class="ss-box warn">[^]*That folder is empty\./);
});

test('the last step says what Set Up starts, in the developer\'s words', () => {
  const c = context();
  const su = copy({ copy: '/roms/G.iso', prepare_label: 'Building the game (10 minutes)', build_where: '/home/p/Games/Hop' });
  const html = c.stepsView('g', su, {}, { name: 'R' });
  assert.match(html, /<span>Then<\/span><b>Building the game \(10 minutes\)<\/b>/);
  assert.match(html, /<span>Builds into<\/span><b>\/home\/p\/Games\/Hop<\/b>/);
});

test('shared keys are their own group and say they are kept once', () => {
  const c = context();
  const su = { needs: 'x', kind: '', inputs: [{ id: 'base', label: 'Your game', role: 'game', file: true, active: true, path: '/g.nsp' }, { id: 'prod', label: 'prod.keys', role: 'key', shared: 'switch.prod-keys', file: true, active: true, path: 'kept' }], prepare_options: [] };
  const html = c.stepsView('g', su, {}, { name: 'R' });
  assert.match(html, /<p class="ss-cap">Keys <i>/);
  assert.match(html, /<span class="ss-src shared">Already on this PC<\/span>/);
});

test('labels and paths from the profile are escaped', () => {
  const c = context();
  const su = { needs: 'x', kind: '', inputs: [{ ...disc(1, '/<img>.iso'), label: '<script>bad</script>' }, disc(2)], prepare_options: [{ id: 'o', label: '<b>x</b>', on: true }] };
  const html = c.stepsView('g', su, {}, { name: '<i>n</i>' }) + c.stepsView('g', su, { at: 1 }, { name: 'n' });
  assert.doesNotMatch(html, /<script>bad|<img>|<b>x<\/b>|<i>n<\/i>/);
});

test('the desktop window and the change of an option draw the steps', () => {
  const app = read('app.js');
  assert.match(app, /stage: 'steps', named: \{ at: 0, error: note \}/);
  assert.match(app, /if \(SU\.stage === 'steps'\) return ssDraw\(\);/);
  assert.match(app, /if \(SU\.stage === 'steps'\) ssDraw\(\);\n\}\);/);
  assert.match(app, /stepsDrop\(id, paths\[0\], SU\.named\)/);
  assert.match(read('index.html'), /<script src="setup-inputs\.js"><\/script>\r?\n<script src="setup-steps\.js"><\/script>/);
});

test('Big Picture opens the same steps: A presses, B goes back a step before it closes', () => {
  const tv = read('tv.js');
  assert.match(tv, /Object\.assign\(TS, \{ id, stage: 'steps', named: \{ at: 0 \}/);
  assert.match(tv, /body = stepsShell\(id, su, TS\.named, a, true\);/);
  assert.match(tv, /b\.dataset\.key = `ss:\$\{act\}:\$\{input\}`;/);
  assert.match(tv, /back: \(\) => \{\n\s+if \(TS\.stage !== 'steps' \|\| !\(TS\.named\.at > 0\)\) return false;\n\s+TS\.named\.at -= 1;/);
  assert.match(tv, /const go = await stepsRun\(id, ns, TS\.optsSaving\);/);
  assert.doesNotMatch(tv, /namedInputsShell|TS\.stage === 'pick'/);
});

test('a game whose setup step kept the copy builds again without it, on both screens', async () => {
  const c = context();
  const su = copy({ kept: true });
  const m = c.stepsModel('g', su, {});
  assert.ok(m.kept);
  assert.equal(m.missing.length, 0, 'what it kept stands in for the copy');
  assert.equal(m.blocked, false);
  const html = c.stepsView('g', su, {}, { name: 'R', game: 'G' });
  assert.match(html, /<h2>Build It Again<\/h2><p>Your copy isn&#39;t needed: it&#39;s built again from what it kept last time\.<\/p>/);
  assert.match(html, /data-ss="file"/, 'the copy can still be picked, to build from it instead');
  assert.match(c.stepsView('g', su, { copy: '/roms/G.nup' }, { name: 'R', game: 'G' }), /<h2>Insert your copy<\/h2>/);
  c.S.setup.g = su;
  assert.deepEqual({ ...(await c.stepsRun('g', {}, null)) }, { kept: true });
  // A copy picked anyway is used: the player chose it.
  assert.deepEqual({ ...(await c.stepsRun('g', { copy: '/roms/G.nup' }, null)) }, { copy: '/roms/G.nup' });
  // A build that's already there still skips the build.
  assert.equal(c.stepsModel('g', copy({ kept: true, prebuilt: '/install/G' }), {}).kept, false);
  const app = read('app.js'), tv = read('tv.js');
  assert.match(app, /if \(go\.kept\) return suBegin\('', true\);/);
  assert.match(tv, /if \(go\.kept\) return tsBegin\('', true\);/);
  assert.match(app, /const order = \(SU\.kept \? \[\] : \['read', 'verify'\]\)/);
  assert.match(tv, /const order = \(TS\.kept \? \[\] : \['read', 'verify'\]\)/);
  assert.match(app, /\/\^NEED_REBUILD\\\|kept\$\/\.test\([^\n]*\) S\.setupNote = null;/);
});
