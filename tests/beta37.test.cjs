const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js'), setup = read('setup-inputs.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const base = () => ({
  t: (x) => x, tf: (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]), tn: (n, one, many) => (n === 1 ? one : many).replace('{n}', n),
  esc: (x) => String(x ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])),
  baseName: (x) => x.split(/[\\/]/).pop(), CHECK: '<svg class="ok"></svg>', DISC_S: '<svg class="no"></svg>',
});

// ---- NVIDIA DLSS row (desktop and Big Picture share namedInputsView) ----
function setupCtx() { const c = vm.createContext(base()); vm.runInContext(setup, c); return c; }
const disc = { id: 'disc', label: 'Disc', role: 'disc', path: '/d.iso', active: true, file: true };
const dlss = (o = {}) => Object.assign({ id: 'dlss', label: 'NVIDIA DLSS', role: 'runtime', shared: 'nvidia.dlss', optional: true, active: true, runtime: true, file: false, path: null, off: false, ships: false }, o);

test('the DLSS row has nothing to choose and never holds setup back', () => {
  const c = setupCtx();
  const html = c.namedInputsView('g', { inputs: [disc, dlss()] }, { extras: true });
  assert.match(html, /NVIDIA DLSS/);
  assert.match(html, /Downloaded from NVIDIA/);
  assert.doesNotMatch(html, /data-ni="file" data-input="dlss"/);
  assert.doesNotMatch(html, /data-ni="run" disabled/, 'setup can run without it');
  assert.match(html, /data-ni="dlss-off"/);
  // Even when a profile marks it required.
  assert.doesNotMatch(c.namedInputsView('g', { inputs: [disc, dlss({ optional: false })] }), /data-ni="run" disabled/);
});
test('the DLSS row shows its download, a failure with Try Again, off, and a shipped file', () => {
  const c = setupCtx();
  const downloading = c.namedInputsView('g', { inputs: [disc, dlss()] }, { extras: true, dlss: { done: 29, total: 58 } });
  assert.match(downloading, /Downloading from NVIDIA 50%/);
  assert.match(downloading, /ni-bar/);
  const failed = c.namedInputsView('g', { inputs: [disc, dlss()] }, { extras: true, dlss: { failed: true } });
  assert.match(failed, /Couldn&#39;t get NVIDIA&#39;s DLSS file\. The game runs without it\./);
  assert.match(failed, /data-ni="dlss-retry"/);
  assert.doesNotMatch(failed, /data-ni="run" disabled/);
  const off = c.namedInputsView('g', { inputs: [disc, dlss({ off: true })] }, { extras: true });
  assert.match(off, /Off for this game/);
  assert.match(off, /data-ni="dlss-on"/);
  const ships = c.namedInputsView('g', { inputs: [disc, dlss({ ships: true })] }, { extras: true });
  assert.match(ships, /Using the DLSS file this game ships/);
  assert.doesNotMatch(ships, /data-ni="dlss-(on|off|retry)"/);
});
test('Big Picture draws the same rows and fetches DLSS the same way', () => {
  assert.match(tv, /namedInputsShell\(id, su, TS\.named, a, true\)/);
  assert.match(tv, /runtimeFetch\(id, TS\.named/);
});

// ---- the scan pill, players, Dreamcast ----
test('the scan pill opens VirusTotal for that file, and ClamAV explains itself', () => {
  const c = vm.createContext(Object.assign(base(), { S: { scans: {}, os: 'windows' }, scanAsk: new Set(), invoke: async () => ({}), document: { querySelectorAll: () => [], body: { classList: { contains: () => false } } }, window: {}, CSS: { escape: (x) => x } }));
  vm.runInContext(fn(app, 'scanSha') + fn(app, 'scanInfo') + fn(app, 'scanPill'), c);
  const sha = 'a'.repeat(64);
  c.S.scans['g@v1'] = [{ verdict: 'yellow', scanner: 'virustotal', found: 3, engines: 70, sha256: sha, for: ['windows'] }];
  const vt = c.scanPill({ id: 'g', tag: 'v1' });
  assert.match(vt, /<button[^>]+data-act="scan-open"/);
  assert.match(vt, new RegExp(`data-sha="${sha}"`));
  assert.match(vt, /3 of 70 engines flagged/);
  assert.match(vt, /usually false positives/);
  c.S.scans['g@v2'] = [{ verdict: 'green', scanner: 'clamav', for: ['windows'] }];
  assert.match(c.scanPill({ id: 'g', tag: 'v2' }), /data-scanner="clamav"/);
  assert.match(app, /virustotal\.com\/gui\/file\/\$\{sha\}/);
  assert.match(app, /Checked by ClamAV on recompstore\.com before it was published\./);
  assert.match(tv, /data-tvact="scan"/);
});
test('players show only when the catalog has them', () => {
  const c = vm.createContext(base());
  vm.runInContext(fn(app, 'playersText'), c);
  assert.equal(c.playersText({}), '');
  assert.equal(c.playersText({ players: 0 }), '');
  assert.equal(c.playersText({ players: 1 }), '1 PC, last 90 days');
  assert.equal(c.playersText({ players: 412 }), '412 PCs, last 90 days');
  assert.match(app, /\[t\('Played on RecompLauncher'\), playersText\(a\)\]/);
  assert.match(tv, /\[t\('Played on RecompLauncher'\), playersText\(a\)\]/);
});

// ---- the Updates tab follows the badge's rule ----
test('the Updates tab offers the same update as the badge', () => {
  const c = vm.createContext(Object.assign(base(), { S: { jobs: {}, running: new Set(), updInfo: { g: { offered: 'v2', checked: 'v2', builds: ['v3', 'v2', 'v1'] } } }, invoke: async () => ({}) }));
  vm.runInContext(fn(app, 'relState'), c);
  const list = [{ tag_name: 'v3' }, { tag_name: 'v2' }, { tag_name: 'v1' }];
  const inst = { tag: 'v1' };
  assert.match(c.relState('g', inst, list, list[0], 0), /Still being checked/);
  assert.match(c.relState('g', inst, list, list[1], 1), /btn sm main[^>]+>Update</);
  assert.match(c.relState('g', inst, list, list[2], 2), /Installed/);
  c.S.updInfo.g.builds = ['v1'];
  assert.match(c.relState('g', inst, list, list[1], 1), /No build for your system/);
});

// ---- Settings → Network ----
test('the proxy setting offers off, the system proxy and a custom address', () => {
  const c = vm.createContext(Object.assign(base(), { S: {} }));
  vm.runInContext(fn(app, 'networkGroup'), c);
  const group = (title, body) => `<g>${title}${body}</g>`, row = (k, d, ctl) => `<r>${k}${ctl}</r>`;
  const sys = c.networkGroup(group, row, { proxy: '' });
  assert.match(sys, /value="system" selected/);
  assert.doesNotMatch(sys, /proxy-url/);
  const custom = c.networkGroup(group, row, { proxy: 'custom', proxy_url: 'socks5://10.0.0.2:1080', proxy_user: 'me', proxy_pass: 'sealed' });
  assert.match(custom, /value="socks5:\/\/10\.0\.0\.2:1080"/);
  assert.match(custom, /id="proxy-pass" type="password"/);
  assert.doesNotMatch(custom, /sealed/, 'the stored password never comes back into the page');
});

// ---- Replaced listings and the Home banner per system ----
function appCtx(S, byId) {
  const c = vm.createContext(Object.assign(base(), { S, byId }));
  vm.runInContext([fn(app, 'replacedBy'), fn(app, 'replacedNote'), fn(app, 'bannerIds')].join('\n'), c);
  return c;
}
test('a replaced listing offers the move only when the new one is listed and not installed', () => {
  const apps = { 'new-recomp': { id: 'new-recomp', name: 'New One' } };
  const S = { cat: { replaced: { 'old-recomp': 'new-recomp' } }, library: { 'old-recomp': { id: 'old-recomp' } } };
  const c = appCtx(S, (id) => apps[id]);
  assert.equal(c.replacedBy('old-recomp').id, 'new-recomp');
  const html = c.replacedNote({ id: 'old-recomp' });
  assert.match(html, /This listing was replaced by New One/);
  assert.match(html, /data-act="move-listing" data-id="old-recomp" data-to="new-recomp"/);
  // Already moved, a game added by hand, a catalog without the key, or a new id it doesn't list: nothing.
  S.library['new-recomp'] = { id: 'new-recomp' };
  assert.equal(c.replacedBy('old-recomp'), null);
  delete S.library['new-recomp'];
  S.library['old-recomp'].local = true;
  assert.equal(c.replacedBy('old-recomp'), null);
  delete S.library['old-recomp'].local;
  S.cat = {};
  assert.equal(c.replacedBy('old-recomp'), null);
  assert.equal(c.replacedNote({ id: 'old-recomp' }), '');
  S.cat = { replaced: { 'old-recomp': 'gone' } };
  assert.equal(c.replacedBy('old-recomp'), null);
});
test('Home takes this system\'s banner list, else the one for all', () => {
  const c = appCtx({}, () => null);
  const cat = { banner: ['a', 'b'], banners: { windows: ['w'], macos: [], linux: ['l1', 'l2'] } };
  assert.deepEqual([...c.bannerIds(cat, 'windows')], ['w']);
  assert.deepEqual([...c.bannerIds(cat, 'linux')], ['l1', 'l2']);
  assert.deepEqual([...c.bannerIds(cat, 'macos')], ['a', 'b'], 'an empty list falls back');
  assert.deepEqual([...c.bannerIds({ banner: ['a'] }, 'linux')], ['a'], 'older catalogs');
  assert.deepEqual([...c.bannerIds({}, 'linux')], []);
  assert.deepEqual([...c.bannerIds({ banners: { linux: 'x' } }, 'linux')], []);
});
test('Big Picture\'s dock and the Library tile follow the same rules', () => {
  assert.match(tv, /bannerApps\(\)/);
  assert.match(tv, /data-tvact="moveto"/);
  assert.match(app, /Replaced by \{name\}/);
});

// ---- Grain over big gradients and blurred art (breaks up colour banding on 8-bit screens) ----
test('one small shared grain tile covers the big gradients and blurred art', () => {
  const png = fs.readFileSync(path.join(__dirname, '../ui/img/noise.png'));
  assert.ok(png.length < 64 * 1024, 'stays small');
  const css = read('app.css'), tvcss = read('tv.css'), ov = read('overlay.css');
  assert.match(css, /--noise:url\(img\/noise\.png\)/);
  for (const sel of ['.gp-amb::after', '.slide::after', '.upd-start', '.dl-hero', '.mc', '.beta-hero']) assert.ok(css.includes(`${sel}{`) && new RegExp(`(^|\\n)${sel.replace(/[.*:]/g, '\\$&')}\\{[^}]*var\\(--noise\\)`).test(css), sel);
  for (const sel of ['.tv-full::after', '.tv-glow', '.tv-boot']) assert.ok(new RegExp(`(^|\\n)${sel.replace(/[.*:]/g, '\\$&')}\\{[^}]*var\\(--noise\\)`).test(tvcss), sel);
  assert.match(tvcss, /\.tv-ambient::after/);
  assert.match(ov, /\.stage\{[^}]*url\(img\/noise\.png\)/);
  // No blend modes or animation on it: plain layers stay cheap to draw.
  assert.doesNotMatch(css + tvcss, /var\(--noise\)[^;}]*;?[^}]*mix-blend-mode/);
});
