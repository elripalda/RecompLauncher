const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), html = read('index.html');

// The Star on GitHub block out of app.js, with a sidebar box and a recording backend.
function load(settings) {
  const start = app.indexOf('// ---------- Star on GitHub ----------');
  const end = app.indexOf('// A recomp\'s own installer or first-time setup');
  assert.ok(start >= 0 && end > start);
  const box = { hidden: true, innerHTML: '', get firstChild() { return this.innerHTML || null; } };
  const calls = { open: [], saved: [] };
  const S = { settings };
  const saveUi = (patch) => { calls.saved.push(patch); S.settings = Object.assign({}, S.settings, { ui: Object.assign({}, S.settings.ui, patch) }); return Promise.resolve(); };
  const invoke = (cmd, args) => { if (cmd === 'open_url') calls.open.push(args.url); return Promise.resolve(); };
  const api = new Function('S', '$', 't', 'esc', 'saveUi', 'invoke', 'fail', 'toast', 'modal',
    `${app.slice(start, end)} return { paintStarCta, starOpen, starBanner, GH_REPO };`)(
    S, () => box, (s) => s, (s) => s, saveUi, invoke, () => {}, () => {}, () => {});
  return { api, box, calls };
}

test('the sidebar shows Star on GitHub once setup is done', () => {
  const { api, box } = load({ setup_done: true, ui: {} });
  api.paintStarCta();
  assert.equal(box.hidden, false);
  assert.match(box.innerHTML, /data-act="star"/);
  assert.match(box.innerHTML, /Star on GitHub/);
});

test('the sidebar button waits for first-time setup', () => {
  const { api, box } = load({ setup_done: false, ui: {} });
  api.paintStarCta();
  assert.equal(box.hidden, true);
});

test('the sidebar button goes once the player has starred or hidden it', () => {
  for (const ui of [{ star_done: 1 }, { star_hide: 1 }]) {
    const { api, box } = load({ setup_done: true, ui });
    api.paintStarCta();
    assert.equal(box.hidden, true);
  }
});

test('Star on GitHub opens the repo and remembers it', async () => {
  const { api, box, calls } = load({ setup_done: true, ui: {} });
  api.paintStarCta();
  api.starOpen();
  await new Promise((ok) => setTimeout(ok, 0));
  assert.deepEqual(calls.open, ['https://github.com/elripalda/RecompLauncher']);
  assert.ok(calls.saved.some((p) => p.star_done));
  assert.equal(box.hidden, true);
});

test('Settings > About always has the banner, and it opens the same window', () => {
  const { api } = load({ setup_done: true, ui: { star_done: 1 } });
  assert.match(api.starBanner(), /data-act="star"/);
  assert.match(app, /\+ starBanner\(\)/);
  assert.match(app, /case 'star': return starModal\(\);/);
});

test('the sidebar button sits above the line over the account', () => {
  const at = html.indexOf('id="star-cta"');
  assert.ok(at > html.indexOf('class="side-scroll"'));
  assert.ok(at < html.indexOf('class="side-foot"'));
});
